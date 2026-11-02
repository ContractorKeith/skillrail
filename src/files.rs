//! Bounded, non-executing reads shared by discovery, metadata and fingerprints.
use std::{fs, io::Read, path::Path};

pub fn read(path: &Path, limit: u64) -> Result<Vec<u8>, ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    // Do not block on a substituted FIFO or follow a substituted final symlink.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(target_os = "linux")]
        options.custom_flags(0x20000 | 0x800); // O_NOFOLLOW | O_NONBLOCK
        #[cfg(target_os = "macos")]
        options.custom_flags(0x100 | 0x4); // O_NOFOLLOW | O_NONBLOCK
    }
    let file = options.open(path).map_err(|_| ())?;
    let opened = file.metadata().map_err(|_| ())?;
    if !opened.is_file() || opened.len() > limit {
        return Err(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
            return Err(());
        }
    }
    let mut bytes = Vec::new();
    (&file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    let after = fs::symlink_metadata(path).map_err(|_| ())?;
    let read_metadata = file.metadata().map_err(|_| ())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != after.dev() || metadata.ino() != after.ino() {
            return Err(());
        }
    }
    if bytes.len() as u64 > limit
        || after.len() != metadata.len()
        || after.modified().ok() != metadata.modified().ok()
        || read_metadata.len() != metadata.len()
        || read_metadata.modified().ok() != metadata.modified().ok()
        || !after.is_file()
    {
        return Err(());
    }
    Ok(bytes)
}

pub fn text(path: &Path, limit: u64) -> Result<String, ()> {
    String::from_utf8(read(path, limit)?).map_err(|_| ())
}

pub fn id(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}
