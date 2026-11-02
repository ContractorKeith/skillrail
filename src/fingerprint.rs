//! Versioned full-folder fingerprints; internal links are recorded, never traversed.
use crate::files;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct Fingerprint {
    pub algorithm: &'static str,
    pub sha256: Option<String>,
    pub diagnostics: Vec<String>,
}

pub fn fingerprint(root: &Path) -> Fingerprint {
    let mut hash = Sha256::new();
    let mut diagnostics = Vec::new();
    let mut entries = 0;
    let mut bytes = 0;
    let mut failure = None;
    hash.update(b"skillrail-folder-v1");
    if walk(
        root,
        root,
        &mut hash,
        &mut entries,
        &mut bytes,
        0,
        &mut failure,
    )
    .is_err()
    {
        let relative = failure
            .as_deref()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy())
            .unwrap_or_default();
        diagnostics.push(format!("fingerprint_incomplete at {}: unreadable, unsafe, changing or over-budget entry; drift is unknown", if relative.is_empty() { "<root>" } else { &relative }));
    }
    Fingerprint {
        algorithm: "skillrail-folder-v1-sha256",
        sha256: diagnostics.is_empty().then(|| {
            hash.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        }),
        diagnostics,
    }
}

fn field(hash: &mut Sha256, data: &[u8]) {
    hash.update((data.len() as u64).to_le_bytes());
    hash.update(data);
}

fn walk(
    root: &Path,
    path: &Path,
    hash: &mut Sha256,
    entries: &mut usize,
    bytes: &mut u64,
    depth: usize,
    failure: &mut Option<PathBuf>,
) -> Result<(), ()> {
    *failure = Some(path.to_owned());
    *entries += 1;
    if *entries > 10_000 || depth > 64 {
        return Err(());
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ())?
        .to_str()
        .ok_or(())?;
    field(hash, relative.as_bytes());
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Execution bits affect scripts; timestamps, UID and write bits do not define content.
        field(hash, &(metadata.permissions().mode() & 0o111).to_le_bytes());
    }
    if metadata.is_symlink() {
        field(hash, b"link");
        let target = fs::read_link(path).map_err(|_| ())?;
        field(hash, target.to_str().ok_or(())?.as_bytes());
        let resolved = path.canonicalize().map_err(|_| ())?;
        if !resolved.starts_with(root)
            || path.starts_with(&resolved)
            || resolved
                .strip_prefix(root)
                .map_err(|_| ())?
                .components()
                .any(|c| matches!(c.as_os_str().to_str(), Some(".git" | ".skillrail")))
        {
            return Err(());
        }
        let target_metadata = fs::metadata(resolved).map_err(|_| ())?;
        if !target_metadata.is_file() && !target_metadata.is_dir() {
            return Err(());
        }
    } else if metadata.is_dir() {
        field(hash, b"directory");
        let mut children = fs::read_dir(path)
            .map_err(|_| ())?
            .take(10_001)
            .map(|entry| entry.map(|entry| entry.path()).map_err(|_| ()))
            .collect::<Result<Vec<_>, _>>()?;
        if children.len() > 10_000 {
            return Err(());
        }
        children.sort();
        for child in children {
            if matches!(
                child.file_name().and_then(|s| s.to_str()),
                Some(".git" | ".skillrail")
            ) {
                continue;
            }
            walk(root, &child, hash, entries, bytes, depth + 1, failure)?;
        }
    } else if metadata.is_file() {
        field(hash, b"file");
        *bytes = bytes.checked_add(metadata.len()).ok_or(())?;
        if *bytes > 64 * 1024 * 1024 {
            return Err(());
        }
        field(hash, &files::read(path, 16 * 1024 * 1024)?);
    } else {
        return Err(());
    }
    Ok(())
}
