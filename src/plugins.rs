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

pub(crate) struct Budget {
    candidates: usize,
    declarations: usize,
    entries: usize,
    reads: usize,
    bytes: u64,
    exhausted: bool,
}
impl Budget {
    fn new() -> Self {
        Self {
            candidates: 128,
            declarations: 256,
            entries: 1000,
            reads: 256,
            bytes: 1024 * 1024,
            exhausted: false,
        }
    }
    fn take(counter: &mut usize, exhausted: &mut bool) -> bool {
        if *counter == 0 {
            *exhausted = true;
            false
        } else {
            *counter -= 1;
            true
        }
    }
    fn candidate(&mut self) -> bool {
        !self.exhausted && Self::take(&mut self.candidates, &mut self.exhausted)
    }
    fn declaration(&mut self) -> bool {
        !self.exhausted && Self::take(&mut self.declarations, &mut self.exhausted)
    }
    fn entry(&mut self) -> bool {
        !self.exhausted && Self::take(&mut self.entries, &mut self.exhausted)
    }
    fn charge(&mut self, path: &Path) -> Result<u64, ()> {
        if self.exhausted || !Self::take(&mut self.reads, &mut self.exhausted) {
            return Err(());
        }
        let len = fs::symlink_metadata(path).map_err(|_| ())?.len();
        if len > self.bytes {
            self.exhausted = true;
            return Err(());
        }
        self.bytes -= len;
        Ok(len)
    }
    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted
    }
    pub(crate) fn json(&mut self, path: &Path) -> Result<Value, ()> {
        let limit = self.charge(path)?.min(128 * 1024);
        crate::json_config::parse(&files::text(path, limit)?)
    }
}

pub fn discover(paths: &Paths, inventory: &mut Inventory) {
    let mut loading = Budget::new();
    let registry = crate::installations::claude(paths, inventory, &mut loading);
    for (agent, root) in [
        (Agent::Claude, paths.home.join(".claude/plugins/cache")),
        (Agent::Codex, paths.home.join(".codex/plugins/cache")),
    ] {
        let mut budget = 1000;
        for market in directories(&root, agent, inventory, &mut budget, &[]) {
            if budget == 0 {
                loading.exhausted = true;
                break;
            }
            for plugin in directories(&market, agent, inventory, &mut budget, &[]) {
                if budget == 0 {
                    loading.exhausted = true;
                    break;
                }
                if agent == Agent::Codex {
                    crate::installations::codex_metadata(&plugin, inventory, &mut loading);
                }
                let versions = directories(
                    &plugin,
                    agent,
                    inventory,
                    &mut budget,
                    registry.as_deref().unwrap_or(&[]),
                );
                let active = if agent == Agent::Codex && budget > 0 {
                    crate::installations::codex_active(&versions)
                } else {
                    None
                };
                for version in versions {
                    let selected: Vec<_> = registry
                        .iter()
                        .flatten()
                        .filter(|r| r.path == version)
                        .collect();
                    if agent == Agent::Claude && !selected.is_empty() {
                        for record in selected {
                            let start = inventory.occurrences.len();
                            let installed_name = version
                                .parent()
                                .and_then(Path::file_name)
                                .and_then(|n| n.to_str());
                            inspect(
                                &version,
                                agent,
                                record.scope,
                                inventory,
                                &mut loading,
                                installed_name,
                            );
                            crate::installations::annotate(
                                inventory,
                                start,
                                "selected",
                                Some(record.version.clone()),
                                Some(paths.home.join(".claude/plugins/installed_plugins.json")),
                            );
                        }
                    } else {
                        let start = inventory.occurrences.len();
                        inspect(
                            &version,
                            agent,
                            Scope::Unknown,
                            inventory,
                            &mut loading,
                            None,
                        );
                        let status = if agent == Agent::Claude {
                            if registry.is_some() {
                                "residual"
                            } else {
                                "unknown"
                            }
                        } else if active.is_none() {
                            "unknown"
                        } else if active.as_ref() == Some(&version) {
                            "selected"
                        } else {
                            "residual"
                        };
                        crate::installations::annotate(
                            inventory,
                            start,
                            status,
                            version
                                .file_name()
                                .and_then(|v| v.to_str())
                                .map(str::to_owned),
                            None,
                        );
                    }
                }
            }
        }
        if budget == 0 {
            loading.exhausted = true;
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
                inspect(&child, Agent::Claude, scope, inventory, &mut loading, None);
            }
        }
    }
    if loading.exhausted {
        inventory.diagnostics.push(crate::Diagnostic{code:"plugin_budget_exceeded".into(),message:"Plugin loading is partial: cumulative candidate, declaration, entry, file-read or byte budget exceeded; final plugin precedence/status is unknown.".into(),location:None});
        for o in &mut inventory.occurrences {
            if matches!(o.origin, Origin::Plugin { .. })
                && let Some(mcp) = &mut o.mcp
            {
                mcp.configured_status = mcp::ConfiguredStatus::Unknown;
            }
        }
    }
    crate::layers::discover(paths, inventory);
}
fn directories(
    path: &Path,
    agent: Agent,
    inventory: &mut Inventory,
    budget: &mut usize,
    installed: &[crate::installations::Record],
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
            Ok(e)
                if e.file_type().is_ok_and(|t| t.is_symlink())
                    && installed.iter().any(|r| r.path == e.path()) =>
            {
                result.push(e.path())
            }
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
fn inspect(
    root: &Path,
    agent: Agent,
    scope: Scope,
    inventory: &mut Inventory,
    loading: &mut Budget,
    installed_name: Option<&str>,
) {
    if !loading.candidate() {
        return;
    }
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
    // Only validated installation evidence supplies identity without a manifest.
    let manifestless = agent == Agent::Claude && installed_name.is_some() && mcp::absent(&manifest);
    if !manifestless
        && !manifest
            .canonicalize()
            .is_ok_and(|p| root.canonicalize().is_ok_and(|r| p.starts_with(r)))
    {
        mcp::problem(
            inventory,
            "plugin_manifest_unknown",
            &manifest,
            agent,
            scope,
        );
        return;
    }
    let parsed = if manifestless {
        Ok(serde_json::json!({"name": installed_name.unwrap()}))
    } else {
        loading.json(&manifest)
    };
    let Ok(value) = parsed else {
        if loading.exhausted {
            return;
        }
        mcp::problem(
            inventory,
            "plugin_manifest_unknown",
            &manifest,
            agent,
            scope,
        );
        return;
    };
    if !value.is_object()
        || !value.get("name").and_then(Value::as_str).is_some_and(|n| {
            !n.is_empty()
                && n.len() <= 128
                && n.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
    {
        mcp::problem(
            inventory,
            "plugin_manifest_unknown",
            &manifest,
            agent,
            scope,
        );
        return;
    }
    let start = inventory.occurrences.len();
    let mut skill_roots = BTreeSet::new();
    if agent == Agent::Claude || portable || value.get("skills").is_none() {
        skill_roots.insert(root.join("skills"));
    }
    if !portable && let Some(paths) = value.get("skills") {
        if let Some(paths) = strings(paths) {
            for text in paths {
                if !loading.declaration() {
                    break;
                }
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
    if agent == Agent::Claude
        && value.get("skills").is_none()
        && mcp::absent(&root.join("skills"))
        && !mcp::absent(&root.join("SKILL.md"))
    {
        skills(
            root, &manifest, root, agent, scope, inventory, true, loading,
        );
    }
    for path in skill_roots {
        if loading.exhausted {
            break;
        }
        skills(
            root, &manifest, &path, agent, scope, inventory, false, loading,
        );
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
        loading,
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
            loading,
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
            o.visibility.reasons.push("Plugin components observed on disk; installation selection and activation observations are reported separately. Native lifecycle is unchanged.".into());
        }
    }
    for o in &mut inventory.occurrences[start..] {
        if let Origin::Plugin { namespace, .. } = &mut o.origin {
            *namespace = value.get("name").and_then(Value::as_str).map(str::to_owned);
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
    loading: &mut Budget,
) {
    let Some(map) = value.as_object().filter(|m| m.len() <= 1000) else {
        *incomplete = true;
        mcp::problem(inventory, "plugin_mcp_unknown", path, agent, Scope::Unknown);
        return;
    };
    for (name, entry) in map {
        if !loading.entry() {
            *incomplete = true;
            break;
        }
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
    loading: &mut Budget,
) {
    if loading.exhausted {
        *incomplete = true;
        return;
    }
    if mcp::absent(path) {
        return;
    }
    match loading.json(path) {
        Ok(value) if value.get("mcpServers").is_some() => merge(
            &value["mcpServers"],
            path,
            agent,
            inventory,
            entries,
            incomplete,
            loading,
        ),
        Ok(value) if !require_wrapper && value.is_object() => {
            merge(&value, path, agent, inventory, entries, incomplete, loading)
        }
        _ => {
            *incomplete = true;
            if !loading.exhausted {
                mcp::problem(inventory, "plugin_mcp_unknown", path, agent, Scope::Unknown);
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn load_declared(
    root: &Path,
    manifest: &Path,
    value: &Value,
    agent: Agent,
    inventory: &mut Inventory,
    entries: &mut Entries,
    incomplete: &mut bool,
    loading: &mut Budget,
) {
    if !loading.declaration() {
        *incomplete = true;
        return;
    }
    if let Some(array) = value.as_array().filter(|a| a.len() <= 1000) {
        for v in array {
            if loading.exhausted {
                break;
            }
            load_declared(
                root, manifest, v, agent, inventory, entries, incomplete, loading,
            );
        }
    } else if let Some(text) = value.as_str() {
        if let Some(path) =
            component(root, text).filter(|p| p.extension().is_some_and(|s| s == "json"))
        {
            load_mcp(&path, agent, inventory, entries, incomplete, false, loading);
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
        merge(
            value, manifest, agent, inventory, entries, incomplete, loading,
        );
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
#[allow(clippy::too_many_arguments)]
fn skills(
    root: &Path,
    manifest: &Path,
    path: &Path,
    agent: Agent,
    scope: Scope,
    inventory: &mut Inventory,
    root_only: bool,
    loading: &mut Budget,
) {
    let mut candidates = if root_only {
        vec![]
    } else {
        directories(path, agent, inventory, &mut loading.entries, &[])
    };
    if loading.entries == 0 {
        loading.exhausted = true;
    }
    if fs::symlink_metadata(path.join("SKILL.md")).is_ok() {
        candidates.push(path.into());
    }
    for original in candidates {
        if !loading.entry() {
            break;
        }
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
        let limit = match loading.charge(&original.join("SKILL.md")) {
            Ok(limit) => limit.min(64 * 1024),
            Err(()) if loading.exhausted => break,
            Err(()) => 0,
        };
        let metadata = crate::discovery::metadata_with_limit(&original, limit).ok();
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
            files::id(&[&format!("{agent:?}/{scope:?}"), &original.to_string_lossy()])
        );
        if inventory.occurrences.iter().any(|o| o.id == occurrence_id) {
            continue;
        }
        let fingerprint = if root_only {
            crate::fingerprint::Fingerprint {
                algorithm: "not_applicable",
                sha256: None,
                diagnostics: vec![
                    "Root fallback fingerprint unknown: unrelated plugin content is not traversed."
                        .into(),
                ],
            }
        } else {
            fingerprint::fingerprint_with_budget(
                canonical.as_ref().unwrap(),
                &mut loading.bytes,
                &mut loading.entries,
                &mut loading.reads,
            )
        };
        if fingerprint.sha256.is_none()
            && !root_only
            && (loading.bytes == 0 || loading.entries == 0 || loading.reads == 0)
        {
            loading.exhausted = true;
        }
        inventory.occurrences.push(Occurrence { id: occurrence_id, item_id: id, name: name.clone(), invocations: vec![], agent, scope, original_path: original, canonical_path: canonical.clone(), visibility: Visibility { status: "unknown", reasons: vec!["Plugin-owned skill observed on disk. Recorded installation/scope and configuration facts do not resolve namespace precedence, runtime trust or session loading.".into()] }, fingerprint, diagnostics: if valid { vec![] } else { vec!["metadata_invalid: plugin skill metadata is incomplete or unsupported".into()] }, origin: Origin::Plugin { root: root.into(), manifest: manifest.into(), configured_status: mcp::ConfiguredStatus::Unknown, installation: crate::installations::Installation::unknown(), namespace: None }, mcp: None, capability: Capability { toggle: false, reason: "M2 is read-only; plugin skill visibility and lifecycle remain owned by the native agent." }, observations: vec![] });
    }
}
