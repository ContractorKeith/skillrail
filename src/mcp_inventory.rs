//! Transient configuration parsing. Only allowlisted facts cross this module's boundary.
use crate::{
    Diagnostic, Inventory, Paths,
    discovery::{Agent, Item, Occurrence, Scope, Visibility},
    files,
    fingerprint::Fingerprint,
    provenance::Source,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    Standalone,
    Plugin {
        root: PathBuf,
        manifest: PathBuf,
        configured_status: ConfiguredStatus,
    },
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfiguredStatus {
    Enabled,
    Disabled,
    Invalid,
    Unknown,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Stdio,
    Http,
    Sse,
    Websocket,
    Unknown,
}
#[derive(Debug, Serialize)]
pub struct Mcp {
    pub configured_status: ConfiguredStatus,
    pub transport: Transport,
}
#[derive(Debug, Serialize)]
pub struct Capability {
    pub toggle: bool,
    pub reason: &'static str,
}
impl Capability {
    pub fn standalone_skill() -> Self {
        Self {
            toggle: false,
            reason: "M2 is read-only; skill visibility mutations are not implemented.",
        }
    }
    fn mcp(plugin: bool) -> Self {
        Self {
            toggle: false,
            reason: if plugin {
                "M2 is read-only; plugin content and lifecycle remain owned by the native agent."
            } else {
                "M2 is read-only; standalone MCP mutations are not implemented."
            },
        }
    }
}

pub(super) fn problem(
    inventory: &mut Inventory,
    code: &str,
    path: &Path,
    agent: Agent,
    scope: Scope,
) {
    inventory.diagnostics.push(Diagnostic {
        code: code.into(),
        message: "Configuration is malformed, inaccessible, over budget or unsupported; raw values and parser errors were discarded.".into(),
        location: Some(crate::DiagnosticLocation { path: path.into(), agent, scope, category: "config_unknown" }),
    });
}
pub(super) fn json(path: &Path) -> Result<Value, ()> {
    serde_json::from_str(&files::text(path, 128 * 1024)?).map_err(|_| ())
}
pub(super) fn config(path: &Path, agent: Agent) -> Result<Value, ()> {
    if agent == Agent::Claude {
        return json(path);
    }
    // Deserialize into a transient tree; never serialize a raw configuration.
    toml::from_str(&files::text(path, 128 * 1024)?).map_err(|_| ())
}
pub(super) fn absent(path: &Path) -> bool {
    // A dangling or inaccessible ancestor is not evidence of absence.
    for parent in path.ancestors() {
        match fs::symlink_metadata(parent) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(_) => return parent != path && fs::metadata(parent).is_ok_and(|m| m.is_dir()),
            Err(_) => return false,
        }
    }
    false
}
pub(super) fn label(name: &str) -> String {
    if !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:@".contains(&b))
    {
        name.into()
    } else {
        "unsupported-name".into()
    }
}
fn map_entries(
    inventory: &mut Inventory,
    value: &Value,
    path: &Path,
    agent: Agent,
    scope: Scope,
    local: bool,
) {
    let key = if agent == Agent::Claude {
        "mcpServers"
    } else {
        "mcp_servers"
    };
    let Some(entries) = value.get(key) else {
        return;
    };
    let Some(entries) = entries.as_object() else {
        problem(inventory, "mcp_schema_unknown", path, agent, scope);
        return;
    };
    if entries.len() > 1000 {
        problem(inventory, "mcp_entry_budget", path, agent, scope);
        return;
    }
    for (name, entry) in entries {
        add_server(
            inventory,
            name,
            entry,
            path,
            agent,
            scope,
            None,
            if local { "claude_local" } else { "standalone" },
        );
    }
}

pub fn discover(paths: &Paths, inventory: &mut Inventory) {
    let claude = paths.home.join(".claude.json");
    if !absent(&claude) {
        match json(&claude) {
            Ok(value) if value.is_object() => {
                map_entries(
                    inventory,
                    &value,
                    &claude,
                    Agent::Claude,
                    Scope::User,
                    false,
                );
                if let Some(projects) = value.get("projects") {
                    if !projects.is_object() {
                        problem(
                            inventory,
                            "mcp_schema_unknown",
                            &claude,
                            Agent::Claude,
                            Scope::Local,
                        );
                    } else if let Some(project) =
                        projects.get(paths.project.to_string_lossy().as_ref())
                    {
                        if !project.is_object() {
                            problem(
                                inventory,
                                "mcp_schema_unknown",
                                &claude,
                                Agent::Claude,
                                Scope::Local,
                            );
                        } else {
                            map_entries(
                                inventory,
                                project,
                                &claude,
                                Agent::Claude,
                                Scope::Local,
                                true,
                            );
                        }
                    }
                }
            }
            _ => problem(
                inventory,
                "mcp_config_unknown",
                &claude,
                Agent::Claude,
                Scope::User,
            ),
        }
    }
    let mut configs = vec![(
        paths.home.join(".codex/config.toml"),
        Agent::Codex,
        Scope::User,
    )];
    for ancestor in crate::discovery::project_ancestors(&paths.project) {
        configs.push((ancestor.join(".mcp.json"), Agent::Claude, Scope::Project));
        configs.push((
            ancestor.join(".codex/config.toml"),
            Agent::Codex,
            Scope::Project,
        ));
    }
    for (path, agent, scope) in configs {
        if absent(&path) {
            continue;
        }
        match config(&path, agent) {
            Ok(value) if value.is_object() => {
                map_entries(inventory, &value, &path, agent, scope, false)
            }
            _ => problem(inventory, "mcp_config_unknown", &path, agent, scope),
        }
    }
    crate::plugins::discover(paths, inventory);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn add_server(
    inventory: &mut Inventory,
    raw_name: &str,
    entry: &Value,
    path: &Path,
    agent: Agent,
    scope: Scope,
    plugin: Option<(&Path, &Path)>,
    selector: &str,
) {
    let name = label(raw_name);
    let identity = files::id(&[
        "mcp-entry",
        &format!("{agent:?}/{scope:?}"),
        &path.to_string_lossy(),
        selector,
        raw_name,
    ]);
    let id = format!("mcp-{identity}");
    let (mut status, transport, mut reason) = sanitize(entry, agent, plugin.is_some());
    if raw_name.is_empty()
        || raw_name.len() > 128
        || !raw_name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        status = ConfiguredStatus::Invalid;
        reason = "Server name is unsupported by the verified schema; unsafe labels are replaced and the raw key is never retained.";
    }
    let source = Source {
        identity,
        evidence: "configuration_occurrence_only; equal server names never establish shared endpoint identity",
        repository: None,
        subpath: None,
        owner: if plugin.is_some() {
            "native_plugin"
        } else {
            "external_configuration"
        },
    };
    inventory.items.push(Item {
        id: id.clone(),
        kind: "mcp",
        name: name.clone(),
        description: None,
        source,
        name_collision: false,
        drift: "unknown",
    });
    inventory.occurrences.push(Occurrence {
        id: format!("occurrence-{id}"), item_id: id, name, invocations: vec![], agent, scope,
        original_path: path.into(), canonical_path: path.canonicalize().ok().filter(|p| p.to_str().is_some()),
        visibility: Visibility { status: "unknown", reasons: vec![reason.into(), "Configured status describes this entry only, not session loading or health. Trust, policy, CLI overrides and configuration precedence remain unresolved; entries are never merged by name.".into(), if agent == Agent::Claude { "Documented Claude precedence: local > project > user > plugin. Project approvals and session opt-outs are not evaluated.".into() } else { "Codex trusted-project configuration can override user configuration; profiles, managed policy and runtime trust are not resolved.".into() }] },
        fingerprint: Fingerprint { algorithm: "not_applicable", sha256: None, diagnostics: vec![] }, diagnostics: vec![],
        origin: plugin.map(|(root, manifest)| Origin::Plugin { root: root.into(), manifest: manifest.into(), configured_status: ConfiguredStatus::Unknown }).unwrap_or(Origin::Standalone),
        mcp: Some(Mcp { configured_status: status, transport }), capability: Capability::mcp(plugin.is_some()),
    });
}
fn sanitize(
    entry: &Value,
    agent: Agent,
    plugin: bool,
) -> (ConfiguredStatus, Transport, &'static str) {
    use ConfiguredStatus::*;
    let invalid = || {
        (
            Invalid,
            Transport::Unknown,
            "Entry does not match a supported server schema; no configuration values are retained.",
        )
    };
    let Some(map) = entry.as_object() else {
        return invalid();
    };
    let kind = map.get("type").and_then(Value::as_str);
    let command = map.get("command");
    let url = map.get("url");
    if map.get("type").is_some_and(|v| !v.is_string()) {
        return invalid();
    }
    if command.is_some() == url.is_some() {
        return invalid();
    }
    if agent == Agent::Claude
        && url.is_some()
        && (kind.is_none() || kind == Some("streamable-http"))
    {
        return invalid();
    }
    if agent == Agent::Codex && !plugin && kind.is_some() {
        return (
            Unknown,
            Transport::Unknown,
            "Codex standalone type field is not part of the supported TOML schema.",
        );
    }
    let transport = match kind {
        Some("stdio") | None if command.is_some() => Transport::Stdio,
        Some("http" | "streamable-http") | None if url.is_some() => Transport::Http,
        Some("sse") if url.is_some() && agent == Agent::Claude => Transport::Sse,
        Some("ws") if url.is_some() && agent == Agent::Claude => Transport::Websocket,
        Some("stdio" | "http" | "streamable-http" | "sse" | "ws") => return invalid(),
        _ => {
            return (
                Unknown,
                Transport::Unknown,
                "Unsupported transport; native validity is unknown.",
            );
        }
    };
    if command.is_some_and(|v| !v.as_str().is_some_and(|s| !s.trim().is_empty())) {
        return invalid();
    }
    if let Some(value) = url {
        let Some(text) = value.as_str() else {
            return invalid();
        };
        // Validate only the structural minimum without retaining any URL fragment.
        let prefixes: &[&str] = if matches!(transport, Transport::Websocket) {
            &["ws://", "wss://"]
        } else {
            &["http://", "https://"]
        };
        if !prefixes.iter().any(|prefix| {
            text.strip_prefix(prefix)
                .is_some_and(|tail| !tail.is_empty() && !tail.starts_with('/'))
        }) || text.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return invalid();
        }
    }
    for key in ["args", "env_vars", "enabled_tools", "disabled_tools"] {
        if map
            .get(key)
            .is_some_and(|v| !v.as_array().is_some_and(|a| a.iter().all(Value::is_string)))
        {
            return invalid();
        }
    }
    for key in ["env", "headers", "http_headers", "env_http_headers"] {
        if map.get(key).is_some_and(|v| {
            !v.as_object()
                .is_some_and(|m| m.values().all(Value::is_string))
        }) {
            return invalid();
        }
    }
    let common = ["command", "args", "env", "url"];
    let agent_fields: &[&str] = if agent == Agent::Claude {
        &[
            "type",
            "headers",
            "headersHelper",
            "oauth",
            "timeout",
            "alwaysLoad",
        ]
    } else {
        &[
            "env_vars",
            "bearer_token_env_var",
            "http_headers",
            "env_http_headers",
            "enabled",
            "startup_timeout_sec",
            "startup_timeout_ms",
            "tool_timeout_sec",
            "enabled_tools",
            "disabled_tools",
            "default_tools_approval_mode",
            "tools",
            "oauth",
            "cwd",
            "scopes",
            "required",
        ]
    };
    let unknown_fields = map.keys().any(|k| {
        !common.contains(&k.as_str())
            && !agent_fields.contains(&k.as_str())
            && !(plugin && ["type", "headers", "auth", "extensions"].contains(&k.as_str()))
    });
    for key in [
        "startup_timeout_sec",
        "startup_timeout_ms",
        "tool_timeout_sec",
        "timeout",
    ] {
        if map
            .get(key)
            .is_some_and(|v| !v.as_f64().is_some_and(|n| n >= 0.0))
        {
            return invalid();
        }
    }
    for key in [
        "bearer_token_env_var",
        "headersHelper",
        "cwd",
        "default_tools_approval_mode",
    ] {
        if map.get(key).is_some_and(|v| !v.is_string()) {
            return invalid();
        }
    }
    for key in ["oauth", "auth", "extensions", "tools"] {
        if map.get(key).is_some_and(|v| !v.is_object()) {
            return invalid();
        }
    }
    for key in ["alwaysLoad", "required", "enabled"] {
        if map.get(key).is_some_and(|v| !v.is_boolean()) {
            return invalid();
        }
    }
    if unknown_fields {
        return (
            Unknown,
            transport,
            "Unknown or agent-incompatible server fields are present; native schema validity and configured status remain unknown.",
        );
    }
    if map
        .keys()
        .any(|key| ["oauth", "auth", "extensions", "tools"].contains(&key.as_str()))
    {
        return (
            Unknown,
            transport,
            "Nested authentication, extension or tool-policy schemas are not evaluated; configured status remains unknown.",
        );
    }
    if agent == Agent::Codex {
        match map.get("enabled") {
            Some(Value::Bool(false)) => {
                return (
                    Disabled,
                    transport,
                    "This Codex entry explicitly configures enabled=false; effective runtime visibility remains unknown.",
                );
            }
            Some(Value::Bool(true)) | None => {}
            _ => return invalid(),
        }
    } else if map.contains_key("enabled") {
        return (
            Unknown,
            transport,
            "Claude entry-level enabled is not a verified enablement mechanism; settings and session opt-outs are unresolved.",
        );
    }
    (
        Enabled,
        transport,
        "Supported transport and required field types; entry is configured on by default. Credentials, command, arguments, environment, headers and URL are omitted.",
    )
}

impl ConfiguredStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Invalid => "invalid",
            Self::Unknown => "unknown",
        }
    }
}
impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::Sse => "sse",
            Self::Websocket => "websocket",
            Self::Unknown => "unknown",
        }
    }
}
