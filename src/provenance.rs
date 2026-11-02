//! Local Git evidence only: no Git commands, includes, hooks or remote requests.
use crate::files;
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Debug, Serialize)]
pub struct Source {
    pub identity: String,
    pub evidence: &'static str,
    pub repository: Option<String>,
    pub subpath: Option<String>,
    pub owner: &'static str,
}

pub fn unresolved(path: &Path) -> Source {
    Source {
        identity: files::id(&["unresolved", &path.to_string_lossy()]),
        evidence: "unresolved_target",
        repository: None,
        subpath: None,
        owner: "external_unknown",
    }
}

pub fn source(path: &Path) -> Source {
    for parent in path.ancestors() {
        let git = parent.join(".git");
        if fs::symlink_metadata(&git).is_err() {
            continue;
        }
        let subpath = path.strip_prefix(parent).ok().and_then(Path::to_str);
        // Linked worktree gitfiles and symlinked Git metadata need separate evidence.
        if fs::symlink_metadata(&git).is_ok_and(|m| m.is_dir())
            && let Ok(config) = files::text(&git.join("config"), 64 * 1024)
            && let Some(remote) = origin(&config)
            && let Some(subpath) = subpath
        {
            return Source {
                identity: files::id(&["git-origin-subpath", &remote, subpath]),
                evidence: "local_git_origin_and_subpath (declared, not authenticated)",
                repository: Some(remote),
                subpath: Some(subpath.into()),
                owner: "external_repository",
            };
        }
        return Source {
            identity: files::id(&["canonical", &path.to_string_lossy()]),
            evidence: "canonical_target; Git metadata unsupported, unavailable or unsafe",
            repository: None,
            subpath: subpath.map(str::to_owned),
            owner: "external_unknown",
        };
    }
    Source {
        identity: files::id(&["canonical", &path.to_string_lossy()]),
        evidence: "canonical_target_only",
        repository: None,
        subpath: None,
        owner: "external_unknown",
    }
}

fn origin(config: &str) -> Option<String> {
    let mut in_origin = false;
    let mut remote = None;
    let mut in_section = false;
    for line in config.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("[include")
            || lower.starts_with("[url")
            || lower.starts_with("[extensions")
            || line.contains('\\')
        {
            return None;
        }
        if line.starts_with('[') {
            let section = line.strip_prefix('[')?.strip_suffix(']')?.trim();
            // Git section/key names are case-insensitive; quoted subsections are not.
            // Reject deprecated dotted remote syntax instead of overlooking another origin.
            if section.to_ascii_lowercase().starts_with("remote.") {
                return None;
            }
            let mut parts = section.splitn(2, char::is_whitespace);
            let kind = parts.next()?;
            let subsection = parts.next().map(str::trim);
            if kind.is_empty() || !kind.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
                return None;
            }
            if let Some(subsection) = subsection {
                let name = subsection.strip_prefix('"')?.strip_suffix('"')?;
                if name.contains('"') {
                    return None;
                }
            }
            in_section = true;
            in_origin = kind.eq_ignore_ascii_case("remote") && subsection == Some("\"origin\"");
        } else {
            let (key, value) = line.split_once('=').map_or((line, None), |(key, value)| {
                (key.trim(), Some(value.trim()))
            });
            // Validate even ignored keys: malformed config is not provenance evidence.
            if !in_section
                || value.is_some_and(|v| v.contains('"'))
                || !key.starts_with(|c: char| c.is_ascii_alphabetic())
                || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return None;
            }
            if in_origin && key.eq_ignore_ascii_case("url") {
                if remote.is_some() {
                    return None;
                }
                remote = Some(safe_remote(value?)?);
            }
        }
    }
    remote
}

// Reject rather than redact credential-bearing or ambiguous URLs. No arbitrary URL fields escape.
fn safe_remote(value: &str) -> Option<String> {
    let rest = value.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if host.is_empty()
        || !host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
    {
        return None;
    }
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !path
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c))
    {
        return None;
    }
    Some(format!("https://{}/{path}", host.to_ascii_lowercase()))
}
