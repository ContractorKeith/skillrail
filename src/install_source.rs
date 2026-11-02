//! Explicit sources only. Git plumbing reads objects in an isolated bare repository.
use crate::{AppError, files, guarded_fs::Directory, mutation::refused};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
mod git;
mod local;
mod metadata;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    pub source: String,
    pub git: bool,
    pub revision: Option<String>,
    pub skill: Option<String>,
    pub agents: Vec<String>,
    pub scope: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub path: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
    pub directory: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub kind: String,
    pub origin: String,
    pub revision: Option<String>,
    pub reference: Option<String>,
    pub subpath: String,
    pub content_hash: String,
}
impl Provenance {
    pub(crate) fn valid(&self) -> bool {
        let hex = |s: &str, len| {
            s.len() == len
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        relative(&self.subpath)
            && hex(&self.content_hash, 64)
            && match self.kind.as_str() {
                "local" => {
                    self.revision.is_none()
                        && self.reference.is_none()
                        && Path::new(&self.origin).is_absolute()
                }
                "git" => {
                    self.reference.as_deref().is_some_and(git_reference)
                        && self.revision.as_ref().is_some_and(|r| hex(r, 40))
                        && (Path::new(&self.origin).is_absolute()
                            || crate::provenance::safe_remote(&self.origin).as_ref()
                                == Some(&self.origin))
                }
                _ => false,
            }
    }
}
pub(crate) struct Source {
    pub entries: Vec<Entry>,
    pub provenance: Provenance,
    pub name: String,
}
fn unsafe_source() -> AppError {
    refused(
        "Source is unsafe, ambiguous, changing, unsupported or over budget; select an explicit skill directory/subpath. Source details were withheld.",
    )
}
pub(crate) fn segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
pub(crate) fn relative(s: &str) -> bool {
    s == "."
        || (!s.is_empty()
            && s.len() <= 1024
            && s.split('/').count() <= 32
            && s.split('/').all(segment))
}
fn git_reference(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && !s.starts_with('-')
        && !s.contains("..")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
}
pub(crate) fn hash(entries: &[Entry]) -> String {
    // Stable sorted representation includes paths, bytes and meaningful executable bits.
    files::id(&[
        "skillrail-snapshot-v1",
        &serde_json::to_string(entries).expect("entry serialization"),
    ])
}
pub(crate) fn collect(r: &InstallRequest) -> Result<Source, AppError> {
    if r.agents.is_empty()
        || r.agents.len() > 2
        || r.agents
            .iter()
            .any(|a| !["claude", "codex"].contains(&a.as_str()))
        || (r.agents.len() == 2 && r.agents[0] == r.agents[1])
        || !["user", "project"].contains(&r.scope.as_str())
        || r.skill.as_deref().is_some_and(|s| !relative(s))
        || (!r.git && r.revision.is_some())
    {
        return Err(AppError::new(
            "invalid_arguments",
            "Select distinct agents, user|project scope, and a contained skill subpath; --ref requires --git.",
        ));
    }
    let (entries, origin, revision, selected) = if r.git {
        git::collect(r)?
    } else {
        let root = std::path::absolute(&r.source).map_err(|_| unsafe_source())?;
        let (entries, selected) = local::collect(&root, r.skill.as_deref())?;
        (
            entries,
            root.to_str().ok_or_else(unsafe_source)?.to_owned(),
            None,
            selected,
        )
    };
    validate_entries(&entries)?;
    let skill = entries
        .iter()
        .find(|e| e.path == "SKILL.md")
        .ok_or_else(unsafe_source)?;
    if skill.bytes.len() > 64 * 1024 {
        return Err(unsafe_source());
    }
    let text = std::str::from_utf8(&skill.bytes).map_err(|_| unsafe_source())?;
    let name = metadata::validate(text, &r.agents)?;
    let content_hash = hash(&entries);
    Ok(Source {
        entries,
        name,
        provenance: Provenance {
            kind: if r.git { "git" } else { "local" }.into(),
            origin,
            revision,
            reference: r
                .git
                .then(|| r.revision.clone().unwrap_or_else(|| "HEAD".into())),
            subpath: selected,
            content_hash,
        },
    })
}

// One selected-tree representation is admitted by every source and journal path.
pub(super) fn validate_entries(entries: &[Entry]) -> Result<(), AppError> {
    let mut nodes = std::collections::BTreeMap::new();
    let mut folded = std::collections::BTreeSet::new();
    let mut bytes = 0usize;
    if entries.is_empty() || entries.len() > 1000 {
        return Err(unsafe_source());
    }
    for entry in entries {
        if entry.path == "."
            || !relative(&entry.path)
            || entry.path.split('/').any(|p| {
                [".git", ".skillrail", ".claude-plugin", ".gitmodules"]
                    .contains(&p.to_ascii_lowercase().as_str())
            })
            || nodes.insert(entry.path.as_str(), entry.directory).is_some()
            || !folded.insert(entry.path.to_ascii_lowercase())
            || (entry.directory && (!entry.bytes.is_empty() || entry.executable))
        {
            return Err(unsafe_source());
        }
        bytes = bytes
            .checked_add(entry.bytes.len())
            .ok_or_else(unsafe_source)?;
        if bytes > 512 * 1024 {
            return Err(unsafe_source());
        }
    }
    for entry in entries {
        for parent in Path::new(&entry.path)
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if nodes.get(parent.to_str().ok_or_else(unsafe_source)?) != Some(&true) {
                return Err(unsafe_source());
            }
        }
    }
    Ok(())
}
