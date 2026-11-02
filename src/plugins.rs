//! Observe bounded plugin caches, without selecting a native installation or following lifecycle hooks.
use crate::{
    Inventory, Paths,
    discovery::{Agent, Item, Occurrence, Scope, Visibility},
    files, fingerprint,
    mcp_inventory::{self as mcp, Capability, Origin},
    provenance::Source,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

pub fn discover(paths: &Paths, inventory: &mut Inventory) {
    for (agent, root) in [
        (Agent::Claude, paths.home.join(".claude/plugins/cache")),
        (Agent::Codex, paths.home.join(".codex/plugins/cache")),
    ] {
        let mut budget = 1000;
        for market in directories(&root, agent, inventory, &mut budget) {
            for plugin in directories(&market, agent, inventory, &mut budget) {
                for version in directories(&plugin, agent, inventory, &mut budget) {
                    inspect(&version, agent, Scope::Unknown, inventory);
                }
            }
        }
    }
    // Claude also documents plugins installed in its skills directory.
    let roots = [
        (Scope::User, paths.home.join(".claude/skills")),
        (Scope::Project, paths.project.join(".claude/skills")),
    ];
    let mut budget = 1000;
    let mut seen = BTreeSet::new();
    for (scope, root) in roots {
        if !seen.insert(root.clone()) {
            continue;
        }
        // M1 already diagnoses unreadable standalone roots. Inspect plugin candidates
        // only, so ordinary skill aliases do not produce plugin-link diagnostics.
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.take(1001).flatten() {
                if budget == 0 {
                    mcp::problem(
                        inventory,
                        "plugin_entry_budget",
                        &root,
                        Agent::Claude,
                        scope,
                    );
                    break;
                }
                budget -= 1;
                let child = entry.path();
                if fs::symlink_metadata(child.join(".claude-plugin")).is_err() {
                    continue;
                }
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    mcp::problem(
                        inventory,
                        "plugin_link_unsupported",
                        &root,
                        Agent::Claude,
                        scope,
                    );
                    continue;
                }
                inspect(&child, Agent::Claude, scope, inventory);
            }
        }
    }
    observations(paths, inventory);
}
fn directories(
    path: &Path,
    agent: Agent,
    inventory: &mut Inventory,
    budget: &mut usize,
) -> Vec<PathBuf> {
    if mcp::absent(path) {
        return vec![];
    }
    if !fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        mcp::problem(
            inventory,
            "plugin_root_unknown",
            path,
            agent,
            Scope::Unknown,
        );
        return vec![];
    }
    let Ok(entries) = fs::read_dir(path) else {
        mcp::problem(
            inventory,
            "plugin_root_unknown",
            path,
            agent,
            Scope::Unknown,
        );
        return vec![];
    };
    let mut result = vec![];
    for entry in entries.take(1001) {
        if *budget == 0 {
            mcp::problem(
                inventory,
                "plugin_entry_budget",
                path,
                agent,
                Scope::Unknown,
            );
            break;
        }
        *budget -= 1;
        match entry {
            Ok(e) if e.path().to_str().is_some() && e.file_type().is_ok_and(|t| t.is_dir()) => {
                result.push(e.path())
            }
            Ok(e) if e.file_type().is_ok_and(|t| t.is_symlink()) => mcp::problem(
                inventory,
                "plugin_link_unsupported",
                path,
                agent,
                Scope::Unknown,
            ),
            Err(_) => mcp::problem(
                inventory,
                "plugin_root_unknown",
                path,
                agent,
                Scope::Unknown,
            ),
            _ => {}
        }
    }
    result.sort();
    result
}
fn component(root: &Path, text: &str) -> Option<PathBuf> {
    if text != "." && !text.starts_with("./") {
        return None;
    }
    let path = Path::new(text);
    if path
        .components()
        .any(|c| !matches!(c, Component::CurDir | Component::Normal(_)))
        || text.contains(['$', '\\'])
    {
        return None;
    }
    let original = root.join(path);
    let canonical = original.canonicalize().ok()?;
    if !canonical.starts_with(root.canonicalize().ok()?) || canonical.to_str().is_none() {
        return None;
    }
    Some(original)
}
fn strings(value: &Value) -> Option<Vec<&str>> {
    if let Some(s) = value.as_str() {
        return Some(vec![s]);
    }
    value.as_array()?.iter().map(Value::as_str).collect()
}
fn inspect(root: &Path, agent: Agent, scope: Scope, inventory: &mut Inventory) {
    let portable = agent == Agent::Codex && !mcp::absent(&root.join("plugin.json"));
    let manifest = if portable {
        root.join("plugin.json")
    } else {
        root.join(if agent == Agent::Claude {
            ".claude-plugin/plugin.json"
        } else {
            ".codex-plugin/plugin.json"
        })
    };
    if !manifest
        .canonicalize()
        .is_ok_and(|p| root.canonicalize().is_ok_and(|r| p.starts_with(r)))
    {
        mcp::problem(
            inventory,
            "plugin_manifest_unknown",
            &root.join("plugin.json"),
            agent,
            scope,
        );
        return;
    }
    let Ok(value) = mcp::json(&manifest) else {
        mcp::problem(
            inventory,
            "plugin_manifest_unknown",
            &manifest,
            agent,
            scope,
        );
        return;
    };
    if !value.is_object() || !value.get("name").is_some_and(Value::is_string) {
        mcp::problem(
            inventory,
            "plugin_manifest_unknown",
            &manifest,
            agent,
            scope,
        );
        return;
    }
    let mut skill_roots = BTreeSet::new();
    if agent == Agent::Claude || portable || value.get("skills").is_none() {
        skill_roots.insert(root.join("skills"));
    }
    if !portable && let Some(paths) = value.get("skills") {
        if let Some(paths) = strings(paths) {
            for text in paths {
                if let Some(path) = component(root, text) {
                    skill_roots.insert(path);
                } else {
                    mcp::problem(
                        inventory,
                        "plugin_component_unknown",
                        &manifest,
                        agent,
                        scope,
                    );
                }
            }
        } else {
            mcp::problem(
                inventory,
                "plugin_component_unknown",
                &manifest,
                agent,
                scope,
            );
        }
    }
    for path in skill_roots {
        skills(root, &manifest, &path, agent, scope, inventory);
    }
    let mut entries = BTreeMap::new();
    let default = root.join(if portable { "mcp.json" } else { ".mcp.json" });
    let mut incomplete = false;
    load_mcp(
        &default,
        agent,
        inventory,
        &mut entries,
        &mut incomplete,
        true,
    );
    if !portable && let Some(declared) = value.get("mcpServers") {
        load_declared(
            root,
            &manifest,
            declared,
            agent,
            inventory,
            &mut entries,
            &mut incomplete,
        );
    }
    if portable {
        // Portable components stay canonical; overlay is not an additional MCP source.
        // Unsupported OpenAI overlays are diagnosed without emitting their raw values.
        if value.get("extensions").is_some_and(|v| !v.is_object()) {
            incomplete = true;
            mcp::problem(inventory, "plugin_overlay_unknown", &manifest, agent, scope);
        }
    }
    for (name, (entry, path)) in entries {
        mcp::add_server(
            inventory,
            &name,
            &entry,
            &path,
            agent,
            scope,
            Some((root, &manifest)),
            "plugin",
        );
        if incomplete && let Some(o) = inventory.occurrences.last_mut() {
            if let Some(m) = &mut o.mcp {
                m.configured_status = mcp::ConfiguredStatus::Unknown;
            }
            o.visibility.reasons.push("A component was unsupported; declaration precedence and final configured status are unknown.".into());
        }
        if let Some(o) = inventory.occurrences.last_mut() {
            o.visibility.reasons.push("Plugin contents observed on disk: cache selection and activation remain unknown; skills-directory scope follows its location. Native lifecycle is unchanged.".into());
        }
    }
}
type Entries = BTreeMap<String, (Value, PathBuf)>;
fn merge(
    value: &Value,
    path: &Path,
    agent: Agent,
    inventory: &mut Inventory,
    entries: &mut Entries,
    incomplete: &mut bool,
) {
    let Some(map) = value.as_object().filter(|m| m.len() <= 1000) else {
        *incomplete = true;
        mcp::problem(inventory, "plugin_mcp_unknown", path, agent, Scope::Unknown);
        return;
    };
    for (name, entry) in map {
        entries.insert(name.clone(), (entry.clone(), path.into()));
    }
}
fn load_mcp(
    path: &Path,
    agent: Agent,
    inventory: &mut Inventory,
    entries: &mut Entries,
    incomplete: &mut bool,
    require_wrapper: bool,
) {
    if mcp::absent(path) {
        return;
    }
    match mcp::json(path) {
        Ok(value) if value.get("mcpServers").is_some() => merge(
            &value["mcpServers"],
            path,
            agent,
            inventory,
            entries,
            incomplete,
        ),
        Ok(value) if !require_wrapper && value.is_object() => {
            merge(&value, path, agent, inventory, entries, incomplete)
        }
        _ => {
            *incomplete = true;
            mcp::problem(inventory, "plugin_mcp_unknown", path, agent, Scope::Unknown);
        }
    }
}
fn load_declared(
    root: &Path,
    manifest: &Path,
    value: &Value,
    agent: Agent,
    inventory: &mut Inventory,
    entries: &mut Entries,
    incomplete: &mut bool,
) {
    if let Some(array) = value.as_array().filter(|a| a.len() <= 1000) {
        for v in array {
            load_declared(root, manifest, v, agent, inventory, entries, incomplete);
        }
    } else if let Some(text) = value.as_str() {
        if let Some(path) =
            component(root, text).filter(|p| p.extension().is_some_and(|s| s == "json"))
        {
            load_mcp(&path, agent, inventory, entries, incomplete, false);
        } else {
            *incomplete = true;
            mcp::problem(
                inventory,
                "plugin_component_unknown",
                manifest,
                agent,
                Scope::Unknown,
            );
        }
    } else if value.is_object() {
        merge(value, manifest, agent, inventory, entries, incomplete);
    } else {
        *incomplete = true;
        mcp::problem(
            inventory,
            "plugin_component_unknown",
            manifest,
            agent,
            Scope::Unknown,
        );
    }
}
fn skills(
    root: &Path,
    manifest: &Path,
    path: &Path,
    agent: Agent,
    scope: Scope,
    inventory: &mut Inventory,
) {
    let mut budget = 1000;
    let mut candidates = directories(path, agent, inventory, &mut budget);
    if fs::symlink_metadata(path.join("SKILL.md")).is_ok() {
        candidates.push(path.into());
    }
    for original in candidates {
        if mcp::absent(&original.join("SKILL.md")) {
            continue;
        }
        let canonical = original.canonicalize().ok().filter(|p| {
            root.canonicalize().is_ok_and(|r| p.starts_with(r)) && p.to_str().is_some()
        });
        if canonical.is_none() {
            mcp::problem(
                inventory,
                "plugin_skill_unknown",
                manifest,
                agent,
                Scope::Unknown,
            );
            continue;
        }
        let metadata = crate::discovery::metadata(&original).ok();
        let valid = metadata.as_ref().is_some_and(|m| {
            agent == Agent::Claude || (m.name.is_some() && m.description.is_some())
        });
        let name = metadata
            .as_ref()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| {
                mcp::label(
                    original
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown"),
                )
            });
        let identity = files::id(&[
            "plugin-skill",
            &root.to_string_lossy(),
            &canonical.as_ref().unwrap().to_string_lossy(),
        ]);
        let id = format!("skill-{identity}");
        if !inventory.items.iter().any(|i| i.id == id) {
            inventory.items.push(Item { id: id.clone(), kind: "skill", name: name.clone(), description: metadata.as_ref().and_then(|m| m.description.clone()), source: Source { identity, evidence: "plugin_root_and_canonical_skill_target; cache presence does not prove activation", repository: None, subpath: None, owner: "native_plugin" }, name_collision: false, drift: "unknown" });
        }
        let occurrence_id = format!(
            "occurrence-{}",
            files::id(&[&format!("{agent:?}"), &original.to_string_lossy()])
        );
        if inventory.occurrences.iter().any(|o| o.id == occurrence_id) {
            continue;
        }
        inventory.occurrences.push(Occurrence { id: occurrence_id, item_id: id, name: name.clone(), invocations: vec![], agent, scope, original_path: original, canonical_path: canonical.clone(), visibility: Visibility { status: "unknown", reasons: vec!["Plugin-owned skill observed on disk. Cache installation scope/version selection or skills-directory activation, namespace, precedence and runtime trust remain unresolved.".into()] }, fingerprint: fingerprint::fingerprint(canonical.as_ref().unwrap()), diagnostics: if valid { vec![] } else { vec!["metadata_invalid: plugin skill metadata is incomplete or unsupported".into()] }, origin: Origin::Plugin { root: root.into(), manifest: manifest.into(), configured_status: mcp::ConfiguredStatus::Unknown }, mcp: None, capability: Capability { toggle: false, reason: "M2 is read-only; plugin skill visibility and lifecycle remain owned by the native agent." } });
    }
}

// These are observations of local rules, never claims about a selected cache version.
fn observations(paths: &Paths, inventory: &mut Inventory) {
    for agent in [Agent::Claude, Agent::Codex] {
        let config_path = paths.home.join(if agent == Agent::Claude {
            ".claude/settings.json"
        } else {
            ".codex/config.toml"
        });
        let user = if mcp::absent(&config_path) {
            Ok(Value::Object(Default::default()))
        } else {
            mcp::config(&config_path, agent)
        };
        let project_present = crate::discovery::project_ancestors(&paths.project)
            .iter()
            .any(|p| {
                if agent == Agent::Codex {
                    !mcp::absent(&p.join(".codex/config.toml"))
                } else {
                    !mcp::absent(&p.join(".claude/settings.json"))
                        || !mcp::absent(&p.join(".claude/settings.local.json"))
                }
            });
        let rule_key = if agent == Agent::Claude {
            "enabledPlugins"
        } else {
            "plugins"
        };
        let valid = user
            .as_ref()
            .is_ok_and(|v| v.is_object() && v.get(rule_key).is_none_or(Value::is_object));
        if !valid {
            mcp::problem(
                inventory,
                "plugin_settings_unknown",
                &config_path,
                agent,
                Scope::User,
            );
        }
        for occurrence in inventory
            .occurrences
            .iter_mut()
            .filter(|o| o.agent == agent)
        {
            let Origin::Plugin {
                root,
                configured_status,
                ..
            } = &mut occurrence.origin
            else {
                continue;
            };
            let cache = paths.home.join(if agent == Agent::Claude {
                ".claude/plugins/cache"
            } else {
                ".codex/plugins/cache"
            });
            if !root
                .strip_prefix(cache)
                .is_ok_and(|p| p.components().count() == 3)
            {
                continue;
            }
            let Some(plugin) = root
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|p| p.to_str())
            else {
                continue;
            };
            let Some(market) = root
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .and_then(|p| p.to_str())
            else {
                continue;
            };
            let key = format!("{plugin}@{market}");
            if !valid || project_present {
                occurrence.visibility.reasons.push("Plugin configuration observations are unknown: malformed settings or project trust/precedence could affect the user rule.".into());
                continue;
            }
            let value = user.as_ref().unwrap();
            let plugin_rule = value.get(rule_key).and_then(|v| v.get(&key));
            let enabled = if agent == Agent::Claude {
                plugin_rule
            } else {
                plugin_rule.and_then(|v| v.get("enabled"))
            };
            if enabled.is_some_and(|v| !v.is_boolean())
                || (agent == Agent::Codex && plugin_rule.is_some_and(|v| !v.is_object()))
            {
                *configured_status = mcp::ConfiguredStatus::Invalid;
                occurrence.diagnostics.push("plugin_rule_invalid: local activation rule has an unsupported type; raw values were discarded".into());
                continue;
            }
            *configured_status = match enabled.and_then(Value::as_bool) {
                Some(true) => mcp::ConfiguredStatus::Enabled,
                Some(false) => mcp::ConfiguredStatus::Disabled,
                None => mcp::ConfiguredStatus::Unknown,
            };
            occurrence.visibility.reasons.push("Plugin configured status observes the matching user rule only; it does not select a cache version or prove activation. Managed/account/session overrides remain unresolved.".into());
            if agent == Agent::Codex
                && let Some(server) = plugin_rule
                    .and_then(|v| v.get("mcp_servers"))
                    .and_then(|v| v.get(&occurrence.name))
                && let Some(mcp) = &mut occurrence.mcp
            {
                if matches!(
                    mcp.configured_status,
                    mcp::ConfiguredStatus::Invalid | mcp::ConfiguredStatus::Unknown
                ) {
                    continue;
                }
                mcp.configured_status = match server.get("enabled").and_then(Value::as_bool) {
                    Some(true) => mcp::ConfiguredStatus::Enabled,
                    Some(false) => mcp::ConfiguredStatus::Disabled,
                    None => mcp::ConfiguredStatus::Unknown,
                };
                occurrence.visibility.reasons.push("Observed user plugin MCP override; effective runtime configuration remains unknown.".into());
            }
        }
    }
}
