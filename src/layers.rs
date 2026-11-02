//! Retain local configuration facts independently of unresolved effective layer precedence.
use crate::{
    Inventory, Paths,
    discovery::{Agent, Occurrence, Scope},
    mcp_inventory::{self as mcp, ConfiguredStatus as Status, Origin},
};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct Observation {
    pub source_path: PathBuf,
    pub scope: Scope,
    pub setting: &'static str,
    pub status: Status,
    pub reason: &'static str,
}
fn observe(o: &mut Occurrence, path: &Path, scope: Scope, setting: &'static str, status: Status) {
    o.observations.push(Observation{source_path:path.into(),scope,setting,status,reason:"Local configuration observation only; effective trust, precedence, managed policy and session selection remain unresolved."});
    if status == Status::Invalid
        && !o
            .diagnostics
            .iter()
            .any(|d| d.starts_with("plugin_rule_invalid"))
    {
        o.diagnostics.push("plugin_rule_invalid: a supported local setting has an invalid container or value; raw values were discarded".into());
    }
}
fn bool_status(v: &Value) -> Status {
    match v.as_bool() {
        Some(true) => Status::Enabled,
        Some(false) => Status::Disabled,
        None => Status::Invalid,
    }
}
fn plugin_key(o: &Occurrence, paths: &Paths) -> Option<String> {
    let Origin::Plugin {
        root, namespace, ..
    } = &o.origin
    else {
        return None;
    };
    let cache = paths.home.join(if o.agent == Agent::Claude {
        ".claude/plugins/cache"
    } else {
        ".codex/plugins/cache"
    });
    if let Ok(relative) = root.strip_prefix(cache) {
        let p: Vec<_> = relative.components().collect();
        if p.len() == 3 {
            return Some(format!(
                "{}@{}",
                p[1].as_os_str().to_str()?,
                p[0].as_os_str().to_str()?
            ));
        }
    }
    if o.agent == Agent::Claude {
        let name = namespace.as_deref()?;
        return Some(format!("{name}@skills-dir"));
    }
    None
}
pub(crate) fn discover(paths: &Paths, inventory: &mut Inventory) {
    for agent in [Agent::Claude, Agent::Codex] {
        let mut layers = vec![(
            paths.home.join(if agent == Agent::Claude {
                ".claude/settings.json"
            } else {
                ".codex/config.toml"
            }),
            Scope::User,
        )];
        for project in crate::discovery::project_ancestors(&paths.project) {
            layers.push((
                project.join(if agent == Agent::Claude {
                    ".claude/settings.json"
                } else {
                    ".codex/config.toml"
                }),
                Scope::Project,
            ));
            if agent == Agent::Claude {
                layers.push((project.join(".claude/settings.local.json"), Scope::Local));
            }
        }
        for (path, scope) in layers {
            if mcp::absent(&path) {
                continue;
            }
            let value = match mcp::config(&path, agent) {
                Ok(v) if v.is_object() => v,
                _ => {
                    mcp::problem(inventory, "plugin_settings_unknown", &path, agent, scope);
                    continue;
                }
            };
            for o in inventory
                .occurrences
                .iter_mut()
                .filter(|o| o.agent == agent)
            {
                if o.mcp.is_none() && matches!(o.origin, Origin::Standalone) {
                    skill_rule(o, &value, &path, scope);
                }
                if agent == Agent::Claude
                    && o.mcp.is_some()
                    && o.scope == Scope::Project
                    && matches!(o.origin, Origin::Standalone)
                {
                    list_choice(o, &value, "disabledMcpjsonServers", &path, scope, true);
                    list_choice(o, &value, "enabledMcpjsonServers", &path, scope, false);
                }
                let Some(key) = plugin_key(o, paths) else {
                    continue;
                };
                let setting = if agent == Agent::Claude {
                    "enabledPlugins"
                } else {
                    "plugins"
                };
                let Some(container) = value.get(setting) else {
                    continue;
                };
                if !container.is_object() {
                    observe(o, &path, scope, "plugin_enabled", Status::Invalid);
                    if o.mcp.is_some() {
                        observe(o, &path, scope, "plugin_mcp_enabled", Status::Invalid);
                    }
                    continue;
                }
                let Some(rule) = container.get(&key) else {
                    continue;
                };
                if agent == Agent::Claude {
                    observe(o, &path, scope, "plugin_enabled", bool_status(rule));
                    continue;
                }
                if !rule.is_object() {
                    observe(o, &path, scope, "plugin_enabled", Status::Invalid);
                    observe(o, &path, scope, "plugin_mcp_enabled", Status::Invalid);
                    continue;
                }
                if let Some(enabled) = rule.get("enabled") {
                    observe(o, &path, scope, "plugin_enabled", bool_status(enabled));
                }
                if o.mcp.is_some()
                    && let Some(servers) = rule.get("mcp_servers")
                {
                    if !servers.is_object() {
                        observe(o, &path, scope, "plugin_mcp_enabled", Status::Invalid);
                        continue;
                    }
                    if let Some(server) = servers.get(&o.name) {
                        if !server.is_object() {
                            observe(o, &path, scope, "plugin_mcp_enabled", Status::Invalid);
                            continue;
                        }
                        let malformed_arrays =
                            ["enabled_tools", "disabled_tools"].iter().any(|key| {
                                server.get(key).is_some_and(|v| {
                                    !v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
                                })
                            });
                        let malformed_approval =
                            server.get("default_tools_approval_mode").is_some_and(|v| {
                                !matches!(
                                    v.as_str(),
                                    Some("auto" | "prompt" | "writes" | "approve")
                                )
                            });
                        let malformed_nested = ["tools", "identity"]
                            .iter()
                            .any(|key| server.get(key).is_some_and(|v| !v.is_object()));
                        if malformed_arrays || malformed_approval || malformed_nested {
                            observe(o, &path, scope, "plugin_mcp_schema", Status::Invalid);
                        } else if server.as_object().unwrap().keys().any(|k| {
                            ![
                                "enabled",
                                "enabled_tools",
                                "disabled_tools",
                                "default_tools_approval_mode",
                            ]
                            .contains(&k.as_str())
                        }) {
                            observe(o, &path, scope, "plugin_mcp_schema", Status::Unknown);
                            if !o
                                .diagnostics
                                .iter()
                                .any(|d| d.starts_with("plugin_rule_unknown"))
                            {
                                o.diagnostics.push("plugin_rule_unknown: nested or unknown override policy is not evaluated".into());
                            }
                        }
                        if let Some(enabled) = server.get("enabled") {
                            observe(o, &path, scope, "plugin_mcp_enabled", bool_status(enabled));
                        }
                    }
                }
            }
        }
    }
    let path = paths.home.join(".claude.json");
    if let Ok(value) = mcp::json(&path)
        && let Some(project) = value
            .get("projects")
            .and_then(|v| v.get(paths.project.to_string_lossy().as_ref()))
    {
        for o in inventory
            .occurrences
            .iter_mut()
            .filter(|o| o.agent == Agent::Claude && o.mcp.is_some())
        {
            list_choice(o, project, "disabledMcpServers", &path, Scope::Local, true);
        }
    }
    for o in &mut inventory.occurrences {
        if let Origin::Plugin {
            configured_status, ..
        } = &mut o.origin
        {
            let states: Vec<_> = o
                .observations
                .iter()
                .filter(|x| x.setting == "plugin_enabled")
                .map(|x| x.status)
                .collect();
            *configured_status = combine(&states).unwrap_or(Status::Unknown);
        }
        if let Some(mcp) = &mut o.mcp {
            if matches!(mcp.configured_status, Status::Invalid | Status::Unknown) {
                continue;
            }
            let states: Vec<_> = o
                .observations
                .iter()
                .filter(|x| {
                    matches!(
                        x.setting,
                        "plugin_mcp_enabled"
                            | "plugin_mcp_schema"
                            | "disabledMcpServers"
                            | "disabledMcpjsonServers"
                    )
                })
                .map(|x| x.status)
                .collect();
            if let Some(status) = combine(&states) {
                mcp.configured_status = status;
            }
        }
    }
}
fn combine(states: &[Status]) -> Option<Status> {
    if states.is_empty() {
        None
    } else if states.contains(&Status::Invalid) {
        Some(Status::Invalid)
    } else if states.iter().any(|s| *s != states[0]) {
        Some(Status::Unknown)
    } else {
        Some(states[0])
    }
}
fn list_choice(
    o: &mut Occurrence,
    value: &Value,
    key: &'static str,
    path: &Path,
    scope: Scope,
    disabled: bool,
) {
    let Some(list) = value.get(key) else { return };
    let Some(list) = list.as_array().filter(|l| l.iter().all(Value::is_string)) else {
        observe(o, path, scope, key, Status::Invalid);
        return;
    };
    // Native plugin server names are namespaced; never disable an unrelated same-name definition.
    let name = if let Origin::Plugin { namespace, .. } = &o.origin {
        namespace
            .as_ref()
            .map(|n| format!("plugin:{n}:{}", o.name))
            .unwrap_or_default()
    } else {
        o.name.clone()
    };
    if list.iter().any(|v| v.as_str() == Some(&name)) {
        observe(
            o,
            path,
            scope,
            key,
            if disabled {
                Status::Disabled
            } else {
                Status::Enabled
            },
        );
    }
}

// These are local settings observations, never proof of effective session visibility.
fn skill_rule(o: &mut Occurrence, value: &Value, path: &Path, scope: Scope) {
    if o.agent == Agent::Claude {
        let Some(rules) = value.get("skillOverrides") else {
            return;
        };
        if !rules.is_object() {
            observe(o, path, scope, "skill_enabled", Status::Invalid);
            return;
        }
        if let Some(rule) = rules.get(&o.name) {
            let status = match rule.as_str() {
                Some("on") => Status::Enabled,
                Some("off") => Status::Disabled,
                Some("name-only" | "user-invocable-only") => Status::Unknown,
                _ => Status::Invalid,
            };
            observe(o, path, scope, "skill_enabled", status);
        }
    } else {
        let Some(rules) = value.get("skills").and_then(|v| v.get("config")) else {
            return;
        };
        let Some(rules) = rules.as_array() else {
            observe(o, path, scope, "skill_enabled", Status::Invalid);
            return;
        };
        let mut last = None;
        for rule in rules {
            let matches = rule.get("name").and_then(Value::as_str).map(str::trim)
                == Some(o.name.as_str())
                || rule.get("path").and_then(Value::as_str).is_some_and(|p| {
                    let p = std::path::Path::new(p);
                    let canonical = p.canonicalize().unwrap_or_else(|_| p.to_owned());
                    o.canonical_path
                        .as_ref()
                        .is_some_and(|c| canonical == c.join("SKILL.md"))
                });
            if matches {
                last = Some(
                    rule.get("enabled")
                        .map(bool_status)
                        .unwrap_or(Status::Invalid),
                );
            }
        }
        if let Some(status) = last {
            observe(o, path, scope, "skill_enabled", status);
        }
    }
}
