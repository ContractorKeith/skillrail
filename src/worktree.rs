//! Claude's documented main-checkout fallback, using bounded local Git layout evidence.
use crate::files;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// None means fallback does not apply. Errors mean its applicability is unknown.
pub fn claude_main_skills(root: &Path) -> Result<Option<PathBuf>, ()> {
    let gitfile = root.join(".git");
    let metadata = match fs::symlink_metadata(&gitfile) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if metadata.is_dir() {
        return Ok(None);
    }
    // A present root skills directory, including an empty one, suppresses fallback.
    let own_skills = root.join(".claude/skills");
    match fs::symlink_metadata(&own_skills) {
        Ok(_) if own_skills.is_dir() => return Ok(None),
        Ok(_) => return Err(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A broken/unsafe .claude parent is not evidence that skills are absent.
            match fs::symlink_metadata(root.join(".claude")) {
                Ok(parent) if parent.is_dir() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(()),
            }
        }
        Err(_) => return Err(()),
    }
    if !metadata.is_file() {
        return Err(());
    }
    let declaration = line(&gitfile)?;
    let target = declaration.strip_prefix("gitdir: ").ok_or(())?;
    let admin = directory(&resolve(root, target)?)?;
    let worktrees = admin.parent().ok_or(())?;
    let common = worktrees.parent().ok_or(())?;
    if worktrees.file_name().and_then(|n| n.to_str()) != Some("worktrees")
        || common.file_name().and_then(|n| n.to_str()) != Some(".git")
    {
        return Err(());
    }
    // Only the normal linked-worktree layout is supported. Both directions must agree.
    let common_pointer = line(&admin.join("commondir"))?;
    if directory(&resolve(&admin, &common_pointer)?)? != common {
        return Err(());
    }
    let backlink = line(&admin.join("gitdir"))?;
    if resolve(&admin, &backlink)? != gitfile {
        return Err(());
    }
    let main = common.parent().ok_or(())?;
    Ok(Some(main.join(".claude/skills")))
}

fn line(path: &Path) -> Result<String, ()> {
    let text = files::text(path, 8192)?;
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(());
    }
    Ok(text.into())
}

fn resolve(base: &Path, value: &str) -> Result<PathBuf, ()> {
    if value.len() > 8192 || value.is_empty() || value.chars().any(char::is_control) {
        return Err(());
    }
    let candidate = base.join(value);
    let mut normalized = PathBuf::new();
    for component in candidate.components().take(257) {
        match component {
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(_) => {
                normalized.push(component.as_os_str());
                // Validate before collapsing later '..' components: symlinks change their meaning.
                if fs::symlink_metadata(&normalized)
                    .map_err(|_| ())?
                    .is_symlink()
                {
                    return Err(());
                }
            }
            Component::CurDir => {}
            Component::ParentDir if normalized.file_name().is_some() => {
                if !fs::symlink_metadata(&normalized).map_err(|_| ())?.is_dir() {
                    return Err(());
                }
                normalized.pop();
            }
            _ => return Err(()),
        }
    }
    if candidate.components().count() > 256 || !normalized.is_absolute() {
        return Err(());
    }
    Ok(normalized)
}

// Reject symlinked administrative paths before opening pointer files. No arbitrary recursion.
fn directory(path: &Path) -> Result<PathBuf, ()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component.as_os_str());
        if !fs::symlink_metadata(&prefix).map_err(|_| ())?.is_dir() {
            return Err(());
        }
    }
    let canonical = path.canonicalize().map_err(|_| ())?;
    if canonical != path || canonical.to_str().is_none() {
        return Err(());
    }
    Ok(canonical)
}
