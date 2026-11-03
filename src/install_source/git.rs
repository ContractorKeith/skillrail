//! Isolated Git acquisition and object reads; no checkout or source configuration.
use super::*;
use std::{
    io::Read,
    process::{Command, Stdio},
};
fn git(repo: &Path, args: &[&str], limit: u64) -> Result<Vec<u8>, AppError> {
    let mut c = Command::new("/usr/bin/git");
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("LC_ALL", "C")
        .env("NETRC", "/dev/null")
        .arg("--git-dir")
        .arg(repo)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "credential.helper=",
            "-c",
            "core.askPass=/bin/false",
            "-c",
            "http.emptyAuth=false",
            "-c",
            "http.delegation=none",
            "-c",
            "http.followRedirects=false",
            "-c",
            "http.proxy=",
            "-c",
            "http.lowSpeedLimit=1",
            "-c",
            "http.lowSpeedTime=20",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    let mut child = c.spawn().map_err(|_| unsafe_source())?;
    let mut bytes = vec![];
    let read = child
        .stdout
        .take()
        .ok_or_else(unsafe_source)?
        .take(limit + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > limit {
        let _ = child.kill();
        let _ = child.wait();
        return Err(unsafe_source());
    }
    if !child.wait().map_err(|_| unsafe_source())?.success() {
        return Err(unsafe_source());
    }
    Ok(bytes)
}
pub(super) fn collect(
    r: &InstallRequest,
) -> Result<(Vec<Entry>, String, Option<String>, String), AppError> {
    let temp = tempfile::Builder::new()
        .prefix("s5g-")
        .tempdir()
        .map_err(|_| unsafe_source())?;
    let repo = temp
        .path()
        .canonicalize()
        .map_err(|_| unsafe_source())?
        .join("repo");
    git(
        &repo,
        &[
            "init",
            "--bare",
            "--template=",
            repo.to_str().ok_or_else(unsafe_source)?,
        ],
        4096,
    )?;
    let reference = r.revision.as_deref().unwrap_or("HEAD");
    if !git_reference(reference) {
        return Err(unsafe_source());
    }
    let origin = if r.source.starts_with("https://") {
        let safe = crate::provenance::safe_remote(&r.source).ok_or_else(unsafe_source)?;
        anonymous_https()?;
        git(
            &repo,
            &[
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                "--",
                &r.source,
                reference,
            ],
            4096,
        )?;
        safe
    } else {
        // Never let Git parse a local repository's config, includes, helpers, hooks or alternates.
        let path = std::path::absolute(&r.source).map_err(|_| unsafe_source())?;
        Directory::open(&path)?;
        let metadata = if path.join(".git").exists() {
            path.join(".git")
        } else {
            path.clone()
        };
        Directory::open(&metadata)?;
        if metadata.join("objects/info/alternates").exists() || metadata.join("shallow").exists() {
            return Err(unsafe_source());
        }
        copy_git_metadata(&metadata, &repo)?;
        path.to_str().ok_or_else(unsafe_source)?.into()
    };
    let selector = if r.source.starts_with("https://") {
        "FETCH_HEAD"
    } else {
        reference
    };
    let rev = String::from_utf8(git(
        &repo,
        &["rev-parse", "--verify", &format!("{selector}^{{commit}}")],
        128,
    )?)
    .map_err(|_| unsafe_source())?
    .trim()
    .to_owned();
    if rev.len() != 40 || !rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(unsafe_source());
    }
    let mut remaining = 10_000usize;
    let selected = match &r.skill {
        Some(path) => path.clone(),
        None => {
            let mut candidates = vec![];
            for entry in listed(&repo, &rev, "-rz", &mut remaining)? {
                let candidate = if entry.path == b"SKILL.md" {
                    Some(b".".as_slice())
                } else {
                    entry.path.strip_suffix(b"/SKILL.md")
                };
                if let Some(path) = candidate {
                    candidates.push(path.to_vec());
                    if candidates.len() > 1 {
                        return Err(unsafe_source());
                    }
                }
            }
            if candidates.len() != 1 {
                return Err(unsafe_source());
            }
            String::from_utf8(candidates.pop().ok_or_else(unsafe_source)?)
                .map_err(|_| unsafe_source())?
        }
    };
    if !relative(&selected) {
        return Err(unsafe_source());
    }
    // Resolve one exact tree component at a time. Never follow a Git symlink or
    // silently pick one of duplicate/case-aliased selected ancestors.
    let mut tree = rev.clone();
    if selected != "." {
        for component in selected.split('/') {
            let level = listed(&repo, &tree, "-z", &mut remaining)?;
            let matches: Vec<_> = level
                .iter()
                .filter(|e| e.path.eq_ignore_ascii_case(component.as_bytes()))
                .collect();
            if matches.len() != 1
                || matches[0].path != component.as_bytes()
                || matches[0].mode != "040000"
                || matches[0].kind != "tree"
            {
                return Err(unsafe_source());
            }
            tree = matches[0].object.clone();
        }
    }
    let listing = listed(&repo, &tree, "-rtz", &mut 1000)?;
    let mut entries = vec![];
    let mut blobs = vec![];
    for entry in listing {
        let directory = match (entry.mode.as_str(), entry.kind.as_str()) {
            ("040000", "tree") => true,
            ("100644" | "100755", "blob") => false,
            _ => return Err(unsafe_source()),
        };
        let path = String::from_utf8(entry.path).map_err(|_| unsafe_source())?;
        entries.push(Entry {
            path,
            bytes: vec![],
            directory,
            executable: entry.mode == "100755",
        });
        blobs.push((entries.len() - 1, entry.object));
    }
    // Shape validation precedes any selected blob read and journal admission.
    validate_entries(&entries)?;
    let mut budget = 512 * 1024usize;
    for (index, object) in blobs {
        if entries[index].directory {
            continue;
        }
        let bytes = git(&repo, &["cat-file", "blob", &object], budget as u64)?;
        budget = budget.checked_sub(bytes.len()).ok_or_else(unsafe_source)?;
        entries[index].bytes = bytes;
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((entries, origin, Some(rev), selected))
}
struct TreeEntry {
    mode: String,
    kind: String,
    object: String,
    path: Vec<u8>,
}
fn listed(
    repo: &Path,
    tree: &str,
    flags: &str,
    remaining: &mut usize,
) -> Result<Vec<TreeEntry>, AppError> {
    let listing = git(repo, &["ls-tree", flags, "--full-tree", tree], 1024 * 1024)?;
    let mut entries = vec![];
    for record in listing.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        *remaining = remaining.checked_sub(1).ok_or_else(unsafe_source)?;
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(unsafe_source)?;
        let meta = std::str::from_utf8(&record[..tab]).map_err(|_| unsafe_source())?;
        let parts: Vec<_> = meta.split(' ').collect();
        if parts.len() != 3
            || parts[2].len() != 40
            || !parts[2].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(unsafe_source());
        }
        entries.push(TreeEntry {
            mode: parts[0].into(),
            kind: parts[1].into(),
            object: parts[2].into(),
            path: record[tab + 1..].to_vec(),
        });
    }
    Ok(entries)
}
fn copy_git_metadata(source: &Path, dest: &Path) -> Result<(), AppError> {
    fn copy(
        source: &Path,
        dest: &Path,
        budget: &mut usize,
        remaining: &mut usize,
        depth: usize,
    ) -> Result<(), AppError> {
        if depth > 32 {
            return Err(unsafe_source());
        }
        let d = Directory::open(source)?;
        let entries = fs::read_dir(source)
            .map_err(|_| unsafe_source())?
            .take(*remaining + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| unsafe_source())?;
        if entries.len() > *remaining {
            return Err(unsafe_source());
        }
        for e in entries {
            *remaining = remaining.checked_sub(1).ok_or_else(unsafe_source)?;
            let n = e.file_name();
            let n = n
                .to_str()
                .filter(|n| segment(n))
                .ok_or_else(unsafe_source)?;
            if ["alternates", "http-alternates"].contains(&n) {
                return Err(unsafe_source());
            }
            let p = e.path();
            let target = dest.join(n);
            if fs::symlink_metadata(&p)
                .map_err(|_| unsafe_source())?
                .is_dir()
            {
                fs::create_dir_all(&target).map_err(|_| unsafe_source())?;
                copy(&p, &target, budget, remaining, depth + 1)?;
            } else {
                let s = d.read(n, false)?.ok_or_else(unsafe_source)?;
                *budget = budget
                    .checked_sub(s.bytes.len())
                    .ok_or_else(unsafe_source)?;
                fs::write(target, s.bytes).map_err(|_| unsafe_source())?;
            }
        }
        Ok(())
    }
    let mut budget = 64 * 1024 * 1024;
    let mut remaining = 10_000usize;
    for n in ["objects", "refs"] {
        let p = source.join(n);
        if p.exists() {
            remaining = remaining.checked_sub(1).ok_or_else(unsafe_source)?;
            copy(&p, &dest.join(n), &mut budget, &mut remaining, 0)?;
        }
    }
    let d = Directory::open(source)?;
    for n in ["HEAD", "packed-refs"] {
        if let Some(s) = d.read(n, false)? {
            remaining = remaining.checked_sub(1).ok_or_else(unsafe_source)?;
            budget = budget
                .checked_sub(s.bytes.len())
                .ok_or_else(unsafe_source)?;
            fs::write(dest.join(n), s.bytes).map_err(|_| unsafe_source())?;
        }
    }
    Ok(())
}

// Git enables CURL_NETRC_OPTIONAL. Older curl ignores NETRC, so absence of the
// OS account's implicit credential files is a prerequisite; no values are read.
fn anonymous_https() -> Result<(), AppError> {
    let mut storage = vec![0u8; 64 * 1024];
    let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    // SAFETY: valid writable passwd/buffer/result storage; geteuid has no arguments.
    let status = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            account.as_mut_ptr(),
            storage.as_mut_ptr().cast(),
            storage.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(unsafe_source());
    }
    // SAFETY: successful getpwuid_r initialized account, and strings borrow storage.
    let account = unsafe { account.assume_init() };
    if account.pw_dir.is_null() {
        return Err(unsafe_source());
    }
    // SAFETY: non-null pw_dir is NUL-terminated within the still-live passwd buffer.
    let home = unsafe { std::ffi::CStr::from_ptr(account.pw_dir) };
    use std::os::unix::ffi::OsStrExt;
    no_netrc(Path::new(std::ffi::OsStr::from_bytes(home.to_bytes())))
}
fn no_netrc(home: &Path) -> Result<(), AppError> {
    if !home.is_absolute()
        || [".netrc", "_netrc"].iter().any(|name| {
            !fs::symlink_metadata(home.join(name))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        })
    {
        return Err(refused(
            "Anonymous HTTPS requires an OS account home without netrc files; Git may otherwise read implicit credentials. No credential values were read. Use a local Git source.",
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_entry_budget_is_shared_across_objects_and_refs() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");
        for root in ["objects", "refs"] {
            fs::create_dir_all(source.join(root)).unwrap();
            fs::create_dir_all(dest.join(root)).unwrap();
            for parent in 0..51 {
                for child in 0..100 {
                    fs::create_dir_all(source.join(root).join(format!("d{parent}/d{child}")))
                        .unwrap();
                }
            }
        }
        assert!(
            copy_git_metadata(&source, &dest).is_err(),
            "aggregate empty metadata directories bypassed the budget"
        );
    }
    #[test]
    fn implicit_http_credential_files_refuse_without_exposing_values_or_following_links() {
        let temp = tempfile::tempdir().unwrap();
        assert!(no_netrc(temp.path()).is_ok());
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "IMPLICIT_CREDENTIAL_CANARY").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join(".netrc")).unwrap();
        let error = no_netrc(temp.path()).unwrap_err();
        assert_eq!(error.code, "mutation_refused");
        assert!(!error.to_string().contains("IMPLICIT_CREDENTIAL_CANARY"));
        assert_eq!(
            fs::read(outside.path()).unwrap(),
            b"IMPLICIT_CREDENTIAL_CANARY"
        );
    }
}
