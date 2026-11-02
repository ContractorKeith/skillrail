//! Versioned installation evidence, separated from enablement and runtime visibility.
use crate::{
    Inventory, Paths,
    discovery::{Agent, Scope},
    mcp_inventory::{self as mcp, Origin},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Installation {
    pub status: &'static str,
    pub version: Option<String>,
    pub evidence_path: Option<PathBuf>,
}
impl Installation {
    pub fn unknown() -> Self {
        Self {
            status: "unknown",
            version: None,
            evidence_path: None,
        }
    }
}
pub(crate) struct Record {
    pub path: PathBuf,
    pub scope: Scope,
    pub version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u32,
    plugins: BTreeMap<String, Vec<serde_json::Value>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawRecord {
    scope: String,
    install_path: PathBuf,
    version: String,
    project_path: Option<PathBuf>,
    installed_at: Option<String>,
    last_updated: Option<String>,
    git_commit_sha: Option<String>,
}
fn segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
}
pub(crate) fn claude(
    paths: &Paths,
    inventory: &mut Inventory,
    loading: &mut crate::plugins::Budget,
) -> Option<Vec<Record>> {
    let path = paths.home.join(".claude/plugins/installed_plugins.json");
    if mcp::absent(&path) {
        return None;
    }
    let mut read = || -> Result<Vec<Record>, ()> {
        let registry: Registry = serde_json::from_value(loading.json(&path)?).map_err(|_| ())?;
        if registry.version != 2 || registry.plugins.len() > 1000 {
            return Err(());
        }
        let mut records = Vec::new();
        let mut count = 0;
        for (key, entries) in registry.plugins {
            let Some((name, market)) = key
                .split_once('@')
                .filter(|(n, m)| segment(n) && segment(m))
            else {
                return Err(());
            };
            for entry in entries {
                count += 1;
                if count > 1000 {
                    return Err(());
                }
                let raw: RawRecord = serde_json::from_value(entry).map_err(|_| ())?;
                let _ = (&raw.installed_at, &raw.last_updated, &raw.git_commit_sha);
                if !segment(&raw.version) || !raw.install_path.is_absolute() {
                    return Err(());
                }
                let expected = paths
                    .home
                    .join(".claude/plugins/cache")
                    .join(market)
                    .join(name)
                    .join(&raw.version);
                if raw.install_path.canonicalize().ok() != expected.canonicalize().ok()
                    || !fs::metadata(&expected).is_ok_and(|m| m.is_dir())
                {
                    return Err(());
                }
                let scope = match raw.scope.as_str() {
                    "user" if raw.project_path.is_none() => Scope::User,
                    "project" => Scope::Project,
                    "local" => Scope::Local,
                    _ => return Err(()),
                };
                if scope != Scope::User {
                    let project = raw.project_path.ok_or(())?;
                    if !project.is_absolute() {
                        return Err(());
                    }
                    if project.canonicalize().ok().as_ref() != Some(&paths.project) {
                        continue;
                    }
                }
                if records
                    .iter()
                    .any(|r: &Record| r.path.parent() == expected.parent() && r.scope == scope)
                {
                    return Err(());
                }
                records.push(Record {
                    path: expected,
                    scope,
                    version: raw.version,
                });
            }
        }
        Ok(records)
    };
    match read() {
        Ok(records) => Some(records),
        Err(()) => {
            mcp::problem(
                inventory,
                "plugin_registry_unknown",
                &path,
                Agent::Claude,
                Scope::Unknown,
            );
            None
        }
    }
}
pub(crate) fn codex_active(versions: &[PathBuf]) -> Option<PathBuf> {
    let mut versions: Vec<_> = versions
        .iter()
        .filter(|p| p.file_name().and_then(|s| s.to_str()).is_some_and(segment))
        .cloned()
        .collect();
    versions.sort_by(|a, b| {
        let a = a.file_name().unwrap().to_str().unwrap();
        let b = b.file_name().unwrap().to_str().unwrap();
        match (semver::Version::parse(a), semver::Version::parse(b)) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            _ => a.cmp(b),
        }
    });
    versions
        .iter()
        .find(|p| p.file_name().is_some_and(|v| v == "local"))
        .cloned()
        .or_else(|| versions.pop())
}
pub(crate) fn annotate(
    inventory: &mut Inventory,
    start: usize,
    status: &'static str,
    version: Option<String>,
    evidence_path: Option<PathBuf>,
) {
    for occurrence in &mut inventory.occurrences[start..] {
        if let Origin::Plugin { installation, .. } = &mut occurrence.origin {
            *installation = Installation {
                status,
                version: version.clone(),
                evidence_path: evidence_path.clone(),
            };
            occurrence.visibility.reasons.push("Installation selection is filesystem/registry evidence only; enablement, trust and live-session visibility remain unresolved.".into());
        }
    }
}

pub(crate) fn codex_metadata(
    base: &std::path::Path,
    inventory: &mut Inventory,
    loading: &mut crate::plugins::Budget,
) {
    let path = base.join(".codex-remote-plugin-install.json");
    if mcp::absent(&path) || loading.exhausted() {
        return;
    }
    let valid = loading.json(&path).is_ok_and(|v| {
        v.as_object().is_some_and(|m| {
            m.keys()
                .all(|k| matches!(k.as_str(), "schema_version" | "remote_plugin_id"))
        }) && v.get("schema_version").and_then(serde_json::Value::as_u64) == Some(1)
            && v.get("remote_plugin_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
    });
    if !valid && !loading.exhausted() {
        mcp::problem(
            inventory,
            "plugin_install_metadata_unknown",
            &path,
            Agent::Codex,
            Scope::User,
        );
    }
}
