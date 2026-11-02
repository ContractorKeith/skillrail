//! Standalone skill adapters. Unsupported runtime restrictions remain explicit uncertainty.
use crate::{
    Diagnostic, Discovery, Inventory, Paths, files,
    fingerprint::{self, Fingerprint},
    provenance::{self, Source},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    Claude,
    Codex,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    User,
    Project,
}
#[derive(Debug, Serialize)]
pub struct Visibility {
    pub status: &'static str,
    pub reasons: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub description: Option<String>,
    pub source: Source,
    pub name_collision: bool,
    pub drift: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Invocation {
    pub name: String,
    pub shadowed_in_default_layout: bool,
}
#[derive(Debug, Serialize)]
pub struct Occurrence {
    pub id: String,
    pub item_id: String,
    pub name: String,
    pub invocations: Vec<Invocation>,
    pub agent: Agent,
    pub scope: Scope,
    pub original_path: PathBuf,
    pub canonical_path: Option<PathBuf>,
    pub visibility: Visibility,
    pub fingerprint: Fingerprint,
    pub diagnostics: Vec<String>,
}
#[derive(Deserialize)]
struct Metadata {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "disable-model-invocation")]
    disable_model_invocation: Option<bool>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
}

fn diagnostic(code: &str, message: &str) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        location: None,
    }
}

fn located(
    message: &str,
    path: &Path,
    agent: Agent,
    scope: Scope,
    category: &'static str,
) -> Diagnostic {
    Diagnostic {
        code: "discovery_incomplete".into(),
        message: message.into(),
        location: Some(crate::DiagnosticLocation {
            path: path.to_owned(),
            agent,
            scope,
            category,
        }),
    }
}

fn access_category(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::NotADirectory => "not_directory",
        std::io::ErrorKind::NotFound => "missing_target",
        _ => "access_failed",
    }
}

pub fn discover(paths: &Paths, timestamp: u64) -> Inventory {
    let mut inventory = Inventory {
        schema_version: crate::SCHEMA_VERSION,
        context: paths.clone(),
        scanned_at_unix_ms: timestamp,
        discovery: Discovery {
            status: "standalone_skills_partial".into(),
            adapters: vec!["claude_standalone_v1".into(), "codex_standalone_v1".into()],
        },
        items: vec![],
        occurrences: vec![],
        diagnostics: vec![diagnostic(
            "coverage_partial",
            "Standalone user/project skills only. Admin, bundled, legacy commands, custom homes, plugins and MCP are not inventoried; runtime policy/trust is unresolved.",
        )],
    };
    let ancestors = project_ancestors(&paths.project);
    let mut roots = vec![
        (
            Agent::Claude,
            Scope::User,
            paths.home.join(".claude/skills"),
        ),
        (Agent::Codex, Scope::User, paths.home.join(".agents/skills")),
    ];
    for parent in &ancestors {
        roots.push((Agent::Claude, Scope::Project, parent.join(".claude/skills")));
        roots.push((Agent::Codex, Scope::Project, parent.join(".agents/skills")));
    }
    let claude_fallback = match ancestors
        .last()
        .map(|root| crate::worktree::claude_main_skills(root))
    {
        Some(Ok(Some(root))) => {
            roots.push((Agent::Claude, Scope::Project, root.clone()));
            Some(root)
        }
        Some(Err(())) => {
            inventory.diagnostics.push(diagnostic("worktree_fallback_unknown", "Claude main-checkout fallback is unknown: Git metadata or the worktree skills root is unsafe, malformed, unsupported or inaccessible."));
            None
        }
        _ => None,
    };
    let codex_config = codex_rules(&paths.home.join(".codex/config.toml"));
    if codex_config.is_err() {
        inventory.diagnostics.push(diagnostic("config_unknown", "Codex user configuration could not be safely interpreted; no raw parser or configuration data is retained."));
    }
    let mut unsupported_config = false;
    for parent in &ancestors {
        if fs::symlink_metadata(parent.join(".codex/config.toml")).is_ok() {
            unsupported_config = true;
        }
    }
    let mut seen = BTreeSet::new();
    for (agent, scope, root) in roots {
        // Home can equal project: the same location is not a second installation.
        if !seen.insert((format!("{agent:?}"), root.clone())) {
            continue;
        }
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && fs::symlink_metadata(&root).is_err() =>
            {
                continue;
            }
            Err(error) => {
                inventory.diagnostics.push(located(
                    "A configured skill root is inaccessible or is not a directory.",
                    &root,
                    agent,
                    scope,
                    access_category(&error),
                ));
                continue;
            }
        };
        let mut children = Vec::new();
        for entry in entries.take(10_001) {
            match entry {
                Ok(entry) => children.push(entry.path()),
                Err(error) => inventory.diagnostics.push(located(
                    "A skill root entry could not be read.",
                    &root,
                    agent,
                    scope,
                    access_category(&error),
                )),
            }
        }
        children.sort();
        if children.len() > 10_000 {
            inventory.diagnostics.push(located(
                "Skill root exceeds the 10000 entry budget.",
                &root,
                agent,
                scope,
                "entry_budget",
            ));
            children.truncate(10_000);
        }
        for original in children {
            if original.to_str().is_none() {
                inventory.diagnostics.push(located(
                    "A non-UTF-8 skill location is unsupported.",
                    &root,
                    agent,
                    scope,
                    "unsupported_encoding",
                ));
                continue;
            }
            let entry_metadata = match fs::symlink_metadata(&original) {
                Ok(metadata) => metadata,
                Err(error) => {
                    inventory.diagnostics.push(located(
                        "A candidate skill location could not be inspected.",
                        &original,
                        agent,
                        scope,
                        access_category(&error),
                    ));
                    continue;
                }
            };
            let link = entry_metadata.is_symlink();
            if !entry_metadata.is_dir() && !link {
                continue;
            }
            let canonical = original
                .canonicalize()
                .ok()
                .filter(|p| p.is_dir() && p.to_str().is_some());
            // A direct ordinary directory without SKILL.md is not a skill. Broken links remain rows.
            if let Some(path) = &canonical {
                match fs::symlink_metadata(path.join("SKILL.md")) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound && !link => {
                        continue;
                    }
                    Err(error) => inventory.diagnostics.push(located(
                        "A candidate skill's metadata is inaccessible; its occurrence remains unknown.",
                        &original, agent, scope, access_category(&error),
                    )),
                    Ok(_) => {}
                }
            }
            if canonical
                .as_ref()
                .is_some_and(|p| p.join(".claude-plugin").exists())
            {
                inventory.diagnostics.push(diagnostic(
                    "plugin_deferred",
                    "A skills-directory plugin was excluded; plugin discovery belongs to M2.",
                ));
                continue;
            }
            let fallback = original
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("invalid-path")
                .to_owned();
            if agent == Agent::Claude && fallback.eq_ignore_ascii_case("synced") {
                inventory.diagnostics.push(diagnostic(
                    "synced_deferred",
                    "Claude account-synced content was excluded; not a standalone skill.",
                ));
                continue;
            }
            let mut problems = Vec::new();
            let metadata = canonical.as_ref().and_then(|p| metadata(p).ok());
            if metadata.is_none() {
                problems.push(
                    "metadata_invalid: missing, malformed, unsafe or over 64 KiB SKILL.md".into(),
                );
            }
            let name = metadata
                .as_ref()
                .and_then(|m| m.name.clone())
                .unwrap_or(fallback.clone());
            let description = metadata.as_ref().and_then(|m| m.description.clone());
            let source = provenance::source(canonical.as_deref().unwrap_or(&original));
            let item_id = format!("skill-{}", source.identity);
            let mut reasons = vec!["Documented standalone discovery location; configured visibility does not prove session loading.".into(), "unknown: managed policy, session flags, custom configuration and trust were not resolved.".into()];
            if agent == Agent::Claude && claude_fallback.as_ref() == Some(&root) {
                reasons.push("Claude main-checkout fallback: linked worktree root has no .claude/skills directory; bounded Git pointers and backlink identify this main checkout. Runtime policy/trust remains unresolved.".into());
            }
            let status = "unknown";
            if agent == Agent::Claude {
                if fallback.starts_with("anthropic-skills") {
                    reasons.push("Reserved Claude skill directory name.".into());
                }
                if metadata
                    .as_ref()
                    .is_some_and(|m| m.disable_model_invocation == Some(true))
                {
                    reasons.push("Frontmatter disables model invocation; explicit invocation may remain available.".into());
                }
                if metadata
                    .as_ref()
                    .is_some_and(|m| m.user_invocable == Some(false))
                {
                    reasons.push(
                        "Frontmatter hides user invocation; model invocation may remain available."
                            .into(),
                    );
                }
                for config in [
                    paths.home.join(".claude/settings.json"),
                    paths.project.join(".claude/settings.json"),
                    paths.project.join(".claude/settings.local.json"),
                ] {
                    if fs::symlink_metadata(config).is_ok() {
                        reasons.push("config_unknown: Claude settings present; permission/visibility and source precedence require runtime policy resolution.".into());
                        break;
                    }
                }
            } else {
                match &codex_config {
                    Ok(rules) if !unsupported_config => {
                        let mut enabled = None;
                        for rule in rules {
                            if rule.name.as_deref() == Some(&name) || rule.path.as_ref().is_some_and(|p| p == &original.join("SKILL.md") || canonical.as_ref().is_some_and(|c| p == &c.join("SKILL.md"))) { enabled = Some(rule.enabled); }
                        }
                        if enabled == Some(false) { reasons.push("User skills.config last matching rule disables this skill; unknown higher-priority runtime policy may override it.".into()); }
                    }
                    _ => reasons.push("config_unknown: Codex configuration malformed, unsupported, inaccessible, or project trust/config unresolved.".into()),
                }
                if metadata
                    .as_ref()
                    .is_some_and(|m| m.name.is_none() || m.description.is_none())
                {
                    problems.push("metadata_invalid: Codex requires name and description".into());
                }
            }
            if canonical.is_none() {
                reasons.push(
                    "Skill target is broken, looping, inaccessible or not a UTF-8 directory."
                        .into(),
                );
            }
            if !problems.is_empty() {
                reasons
                    .push("Metadata or target is incomplete; discoverability is unknown.".into());
            }
            let fingerprint = canonical
                .as_ref()
                .map(|p| fingerprint::fingerprint(p))
                .unwrap_or(Fingerprint {
                    algorithm: "skillrail-folder-v1-sha256",
                    sha256: None,
                    diagnostics: vec![
                        "fingerprint_incomplete: no canonical skill directory".into(),
                    ],
                });
            if !inventory.items.iter().any(|i| i.id == item_id) {
                inventory.items.push(Item {
                    id: item_id.clone(),
                    kind: "skill",
                    name: name.clone(),
                    description,
                    source,
                    name_collision: false,
                    drift: "unknown",
                });
            }
            let mut invocation_names = BTreeSet::from([name.clone()]);
            if agent == Agent::Claude {
                invocation_names.insert(fallback);
            }
            let invocations = invocation_names
                .into_iter()
                .map(|name| Invocation {
                    name,
                    shadowed_in_default_layout: false,
                })
                .collect();
            inventory.occurrences.push(Occurrence {
                id: format!(
                    "occurrence-{}",
                    files::id(&[
                        &format!("{agent:?}"),
                        &format!("{scope:?}"),
                        &original.to_string_lossy()
                    ])
                ),
                item_id,
                name,
                invocations,
                agent,
                scope,
                original_path: original,
                canonical_path: canonical,
                visibility: Visibility { status, reasons },
                fingerprint,
                diagnostics: problems,
            });
        }
    }
    let claude_user_names: BTreeSet<String> = inventory
        .occurrences
        .iter()
        .filter(|o| o.agent == Agent::Claude && o.scope == Scope::User)
        .flat_map(|o| o.invocations.iter().map(|i| i.name.clone()))
        .collect();
    for occurrence in &mut inventory.occurrences {
        if occurrence.agent == Agent::Claude && occurrence.scope == Scope::Project {
            for invocation in &mut occurrence.invocations {
                if claude_user_names.contains(&invocation.name) {
                    invocation.shadowed_in_default_layout = true;
                    occurrence.visibility.reasons.push(format!("shadowed_in_default_layout: Claude personal skill has precedence for invocation /{} only; this does not suppress other invocation names of this skill. Runtime restrictions unresolved.", invocation.name));
                }
            }
        }
    }
    let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for occurrence in &inventory.occurrences {
        names
            .entry(occurrence.name.clone())
            .or_default()
            .insert(occurrence.item_id.clone());
    }
    for item in &mut inventory.items {
        item.name_collision = inventory
            .occurrences
            .iter()
            .any(|o| o.item_id == item.id && names[&o.name].len() > 1);
        let occurrences: Vec<_> = inventory
            .occurrences
            .iter()
            .filter(|o| o.item_id == item.id)
            .collect();
        let sources: BTreeSet<_> = occurrences
            .iter()
            .filter_map(|o| o.canonical_path.as_ref())
            .collect();
        let hashes: BTreeSet<_> = occurrences
            .iter()
            .filter_map(|o| o.fingerprint.sha256.as_ref())
            .collect();
        item.drift = if occurrences.iter().any(|o| o.fingerprint.sha256.is_none()) {
            "unknown"
        } else if sources.len() < 2 {
            "single_source"
        } else if hashes.len() > 1 {
            "drifted"
        } else {
            "equal"
        };
    }
    inventory.items.sort_by(|a, b| a.id.cmp(&b.id));
    inventory.occurrences.sort_by(|a, b| a.id.cmp(&b.id));
    inventory
}

pub fn project_ancestors(project: &Path) -> Vec<PathBuf> {
    let mut result = vec![];
    // Without a repository boundary, only the explicit selected project is supported.
    let boundary = project
        .ancestors()
        .find(|p| fs::symlink_metadata(p.join(".git")).is_ok());
    for parent in project.ancestors() {
        result.push(parent.to_path_buf());
        if boundary.is_none() || Some(parent) == boundary {
            break;
        }
    }
    result
}

fn metadata(path: &Path) -> Result<Metadata, ()> {
    let text = files::text(&path.join("SKILL.md"), 64 * 1024)?;
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err(());
    }
    let mut front = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        front.push_str(line);
        front.push('\n');
    }
    if !closed {
        return Err(());
    }
    let parsed: Metadata = serde_saphyr::from_str(&front).map_err(|_| ())?;
    if parsed.name.as_ref().is_some_and(|s| {
        s.is_empty()
            || s.len() > 128
            || !s
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    }) || parsed.description.as_ref().is_some_and(|s| s.len() > 4096)
    {
        return Err(());
    }
    Ok(parsed)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    name: Option<String>,
    path: Option<PathBuf>,
    enabled: bool,
}
fn codex_rules(path: &Path) -> Result<Vec<Rule>, ()> {
    if !path.try_exists().map_err(|_| ())? {
        return Ok(vec![]);
    }
    let text = files::text(path, 128 * 1024)?;
    let config: toml::Value = toml::from_str(&text).map_err(|_| ())?;
    let Some(skills) = config.get("skills") else {
        return Ok(vec![]);
    };
    let Some(entries) = skills.get("config") else {
        return Ok(vec![]);
    };
    let rules: Vec<Rule> = entries.clone().try_into().map_err(|_| ())?;
    if rules.iter().any(|r| {
        (r.name.is_some() == r.path.is_some()) || r.path.as_ref().is_some_and(|p| !p.is_absolute())
    }) {
        return Err(());
    }
    Ok(rules)
}
