//! Explicit sources only. Git plumbing reads objects in an isolated bare repository.
use crate::{AppError, files, guarded_fs::Directory, mutation::refused};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
mod git;

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
    let (all, origin, revision) = if r.git {
        git::collect(r)?
    } else {
        // Canonical temp ancestors are accepted, but source components themselves cannot be aliases.
        let p = std::path::absolute(&r.source).map_err(|_| unsafe_source())?;
        let d = Directory::open(&p)?;
        let first = local_entries(&p)?;
        if Directory::open(&p)?.chain != d.chain || local_entries(&p)? != first {
            return Err(unsafe_source());
        }
        (
            first,
            p.to_str().ok_or_else(unsafe_source)?.to_owned(),
            None,
        )
    };
    let candidates: Vec<_> = all
        .iter()
        .filter_map(|e| {
            if e.path == "SKILL.md" {
                Some(".".to_owned())
            } else {
                e.path.strip_suffix("/SKILL.md").map(str::to_owned)
            }
        })
        .collect();
    let selected = match &r.skill {
        Some(s) if candidates.contains(s) => s.clone(),
        None if candidates.len() == 1 => candidates[0].clone(),
        _ => return Err(unsafe_source()),
    };
    let prefix = if selected == "." {
        String::new()
    } else {
        format!("{selected}/")
    };
    let entries: Vec<_> = all
        .into_iter()
        .filter_map(|mut e| {
            e.path.strip_prefix(&prefix).map(str::to_owned).map(|p| {
                e.path = p;
                e
            })
        })
        .collect();
    let skill = entries
        .iter()
        .find(|e| e.path == "SKILL.md")
        .ok_or_else(unsafe_source)?;
    if skill.bytes.len() > 64 * 1024 {
        return Err(unsafe_source());
    }
    let text = std::str::from_utf8(&skill.bytes).map_err(|_| unsafe_source())?;
    let m = crate::discovery::parse_metadata(text).map_err(|_| unsafe_source())?;
    let name = m
        .name
        .filter(|s| {
            segment(s)
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .ok_or_else(unsafe_source)?;
    if m.description.is_none_or(|s| s.is_empty() || s.len() > 4096)
        || name.eq_ignore_ascii_case("synced")
        || name.starts_with("anthropic-skills")
    {
        return Err(unsafe_source());
    }
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
pub(crate) fn local_entries(root: &Path) -> Result<Vec<Entry>, AppError> {
    fn walk(
        root: &Path,
        path: &Path,
        out: &mut Vec<Entry>,
        budget: &mut usize,
        depth: usize,
    ) -> Result<(), AppError> {
        if depth > 32 {
            return Err(unsafe_source());
        }
        let d = Directory::open(path)?;
        let mut names = fs::read_dir(path)
            .map_err(|_| unsafe_source())?
            .take(1002)
            .map(|e| e.map(|e| e.file_name()).map_err(|_| unsafe_source()))
            .collect::<Result<Vec<_>, _>>()?;
        names.sort();
        if names.len() > 1000 {
            return Err(unsafe_source());
        }
        let mut folded = std::collections::BTreeSet::new();
        for n in names {
            let n = n.to_str().ok_or_else(unsafe_source)?;
            if n == ".git" {
                continue;
            }
            if n.eq_ignore_ascii_case(".git")
                || !segment(n)
                || !folded.insert(n.to_ascii_lowercase())
                || [".skillrail", ".claude-plugin", ".gitmodules"]
                    .contains(&n.to_ascii_lowercase().as_str())
            {
                return Err(unsafe_source());
            }
            let p = path.join(n);
            let meta = fs::symlink_metadata(&p).map_err(|_| unsafe_source())?;
            if meta.is_dir() {
                out.push(Entry {
                    path: p
                        .strip_prefix(root)
                        .map_err(|_| unsafe_source())?
                        .to_str()
                        .ok_or_else(unsafe_source)?
                        .into(),
                    bytes: vec![],
                    executable: false,
                    directory: true,
                });
                if out.len() > 1000 {
                    return Err(unsafe_source());
                }
                walk(root, &p, out, budget, depth + 1)?;
            } else {
                let s = d.read(n, false)?.ok_or_else(unsafe_source)?;
                *budget = budget
                    .checked_sub(s.bytes.len())
                    .ok_or_else(unsafe_source)?;
                out.push(Entry {
                    path: p
                        .strip_prefix(root)
                        .map_err(|_| unsafe_source())?
                        .to_str()
                        .ok_or_else(unsafe_source)?
                        .into(),
                    directory: false,
                    executable: s.mode() & 0o111 != 0,
                    bytes: s.bytes,
                });
                if out.len() > 1000 {
                    return Err(unsafe_source());
                }
            }
        }
        if Directory::open(path)?.chain != d.chain {
            return Err(unsafe_source());
        }
        Ok(())
    }
    let mut out = vec![];
    walk(root, root, &mut out, &mut (512 * 1024), 0)?;
    if out.iter().any(|e| !relative(&e.path)) {
        return Err(unsafe_source());
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}
