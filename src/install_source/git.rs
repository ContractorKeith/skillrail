//! Isolated Git acquisition and object reads; no checkout or source configuration.
use super::*;
use std::{
    collections::BTreeMap,
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
) -> Result<(Vec<Entry>, String, Option<String>), AppError> {
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
    let listing = git(&repo, &["ls-tree", "-rtz", "--full-tree", &rev], 256 * 1024)?;
    let mut entries = vec![];
    let mut budget = 512 * 1024usize;
    let mut folded = BTreeMap::new();
    for line in listing.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let line = std::str::from_utf8(line).map_err(|_| unsafe_source())?;
        let (meta, path) = line.split_once('\t').ok_or_else(unsafe_source)?;
        let parts: Vec<_> = meta.split(' ').collect();
        if parts.len() != 3
            || !["100644", "100755", "040000"].contains(&parts[0])
            || !["blob", "tree"].contains(&parts[1])
            || !relative(path)
            || path.split('/').any(|s| {
                [".git", ".skillrail", ".claude-plugin", ".gitmodules"]
                    .contains(&s.to_ascii_lowercase().as_str())
            })
        {
            return Err(unsafe_source());
        }
        // Every path prefix has one ASCII spelling on case-insensitive filesystems.
        let mut prefix = String::new();
        for s in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(s);
            if folded
                .insert(prefix.to_ascii_lowercase(), prefix.clone())
                .is_some_and(|v| v != prefix)
            {
                return Err(unsafe_source());
            }
        }
        let directory = parts[0] == "040000";
        let bytes = if directory {
            vec![]
        } else {
            git(&repo, &["cat-file", "blob", parts[2]], budget as u64)?
        };
        budget = budget.checked_sub(bytes.len()).ok_or_else(unsafe_source)?;
        entries.push(Entry {
            path: path.into(),
            bytes,
            executable: parts[0] == "100755",
            directory,
        });
        if entries.len() > 1000 {
            return Err(unsafe_source());
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((entries, origin, Some(rev)))
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
