//! Bounded candidate enumeration precedes reading only the selected local tree.
use super::*;

pub(super) fn collect(
    root: &Path,
    explicit: Option<&str>,
) -> Result<(Vec<Entry>, String), AppError> {
    Directory::open(root)?;
    let selected = match explicit {
        Some(s) => s.to_owned(),
        None => {
            let mut candidates = vec![];
            discover(root, root, &mut candidates, &mut 10_000, 0)?;
            if candidates.len() != 1 {
                return Err(unsafe_source());
            }
            candidates.pop().ok_or_else(unsafe_source)?
        }
    };
    if !relative(&selected) {
        return Err(unsafe_source());
    }
    let path = if selected == "." {
        root.to_owned()
    } else {
        root.join(&selected)
    };
    let before = Directory::open(&path)?.chain;
    let entries = local_entries(&path)?;
    if Directory::open(&path)?.chain != before || local_entries(&path)? != entries {
        return Err(unsafe_source());
    }
    Ok((entries, selected))
}
fn discover(
    root: &Path,
    path: &Path,
    candidates: &mut Vec<String>,
    remaining: &mut usize,
    depth: usize,
) -> Result<(), AppError> {
    if depth > 32 {
        return Err(unsafe_source());
    }
    let directory = Directory::open(path)?;
    let mut entries = fs::read_dir(path)
        .map_err(|_| unsafe_source())?
        .take(*remaining + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| unsafe_source())?;
    if entries.len() > *remaining {
        return Err(unsafe_source());
    }
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        *remaining = remaining.checked_sub(1).ok_or_else(unsafe_source)?;
        if entry.file_name() == ".git" || entry.file_name() == ".skillrail" {
            continue;
        }
        let kind = entry.file_type().map_err(|_| unsafe_source())?;
        if entry.file_name() == "SKILL.md" && !kind.is_dir() {
            let subpath = path.strip_prefix(root).map_err(|_| unsafe_source())?;
            candidates.push(if subpath.as_os_str().is_empty() {
                ".".into()
            } else {
                subpath.to_str().ok_or_else(unsafe_source)?.into()
            });
            if candidates.len() > 1 {
                return Err(unsafe_source());
            }
        } else if kind.is_dir() {
            discover(root, &entry.path(), candidates, remaining, depth + 1)?;
        }
    }
    if Directory::open(path)?.chain != directory.chain {
        return Err(unsafe_source());
    }
    Ok(())
}
fn local_entries(root: &Path) -> Result<Vec<Entry>, AppError> {
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
