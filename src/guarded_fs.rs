//! Descriptor-relative Unix I/O. Recovery bytes are never read through unchecked aliases.
use crate::{AppError, mutation::refused};
use serde::{Deserialize, Serialize};
use std::{
    ffi::CString,
    fs::{File, Metadata},
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path},
};

fn io_error() -> AppError {
    AppError::new(
        "mutation_io",
        "Cannot access or durably write guarded files; inspect private journal history before retrying.",
    )
}
pub(crate) fn unsafe_path() -> AppError {
    refused(
        "Unsafe path, alias, owner, permissions or extended ACL; use regular single-link files and trusted directories. No external target was followed.",
    )
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Identity {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
}
fn identity(m: &Metadata) -> Identity {
    Identity {
        dev: m.dev(),
        ino: m.ino(),
        mode: m.mode(),
        uid: m.uid(),
        gid: m.gid(),
    }
}
fn owned(m: &Metadata) -> bool {
    // SAFETY: geteuid has no arguments or pointer requirements.
    m.uid() == unsafe { libc::geteuid() }
}
fn cstr(s: &std::ffi::OsStr) -> Result<CString, AppError> {
    CString::new(s.as_bytes()).map_err(|_| unsafe_path())
}
fn open_at(parent: &File, name: &std::ffi::OsStr, flags: i32, mode: u32) -> std::io::Result<File> {
    let name = CString::new(name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // C variadic arguments require integer promotion; Darwin mode_t is only u16.
    let promoted_mode: libc::c_uint = mode;
    // SAFETY: parent remains open; name is NUL-terminated, flags/mode are OS constants.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            promoted_mode,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful openat returns a new descriptor owned by this File.
    Ok(unsafe { File::from_raw_fd(fd) })
}
pub(crate) struct Directory {
    file: File,
    pub(crate) chain: Vec<Identity>,
}
impl Directory {
    pub(crate) fn open(path: &Path) -> Result<Self, AppError> {
        if !path.is_absolute() {
            return Err(unsafe_path());
        }
        let mut file = File::open("/").map_err(|_| io_error())?;
        let mut chain = vec![identity(&file.metadata().map_err(|_| io_error())?)];
        for part in path.components() {
            let Component::Normal(name) = part else {
                if part == Component::RootDir {
                    continue;
                }
                return Err(unsafe_path());
            };
            file = open_at(&file, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
                .map_err(|_| unsafe_path())?;
            let m = file.metadata().map_err(|_| io_error())?;
            // Root-owned sticky temporary roots are allowed; descendants must be trusted.
            let sticky_root = m.uid() == 0 && m.mode() & 0o1000 != 0;
            if (!owned(&m) && m.uid() != 0) || (m.mode() & 0o022 != 0 && !sticky_root) {
                return Err(unsafe_path());
            }
            no_extended_acl(&file)?;
            chain.push(identity(&m));
        }
        Ok(Self { file, chain })
    }
    pub(crate) fn private_child(&self, name: &str) -> Result<Self, AppError> {
        let c = cstr(name.as_ref())?;
        // SAFETY: directory fd and NUL-terminated component remain valid throughout call.
        if unsafe { libc::mkdirat(self.file.as_raw_fd(), c.as_ptr(), 0o700) } < 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(io_error());
        }
        let file = open_at(
            &self.file,
            name.as_ref(),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )
        .map_err(|_| unsafe_path())?;
        let m = file.metadata().map_err(|_| io_error())?;
        if !owned(&m) || m.mode() & 0o777 != 0o700 {
            return Err(unsafe_path());
        }
        no_extended_acl(&file)?;
        self.file.sync_all().map_err(|_| io_error())?;
        let mut chain = self.chain.clone();
        chain.push(identity(&m));
        Ok(Self { file, chain })
    }
    pub(crate) fn read(&self, name: &str, private: bool) -> Result<Option<Snapshot>, AppError> {
        let file = match open_at(&self.file, name.as_ref(), libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(unsafe_path()),
        };
        let m = file.metadata().map_err(|_| io_error())?;
        if !m.is_file()
            || !owned(&m)
            || m.nlink() != 1
            || m.mode() & 0o022 != 0
            || (private && m.mode() & 0o777 != 0o600)
            || m.len() > 4 * 1024 * 1024
        {
            return Err(unsafe_path());
        }
        no_extended_acl(&file)?;
        let mut bytes = vec![];
        (&file)
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| io_error())?;
        let after = file.metadata().map_err(|_| io_error())?;
        if bytes.len() as u64 != m.len()
            || after.nlink() != 1
            || after.mtime() != m.mtime()
            || after.mtime_nsec() != m.mtime_nsec()
            || after.ctime() != m.ctime()
            || after.ctime_nsec() != m.ctime_nsec()
        {
            return Err(unsafe_path());
        }
        Ok(Some(Snapshot {
            identity: identity(&m),
            bytes,
        }))
    }
    pub(crate) fn create(
        &self,
        name: &str,
        bytes: &[u8],
        mode: u32,
        group: Option<u32>,
    ) -> Result<(), AppError> {
        let mut file = open_at(
            &self.file,
            name.as_ref(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
        .map_err(|_| unsafe_path())?;
        no_extended_acl(&file)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| io_error())?;
        // The containing staging directory remains private while original metadata is restored.
        if let Some(group) = group {
            // SAFETY: live descriptor; -1 preserves the current owner, gid came from the original file.
            if unsafe { libc::fchown(file.as_raw_fd(), !0 as libc::uid_t, group as libc::gid_t) }
                < 0
            {
                return Err(io_error());
            }
        }
        // Preserve ordinary permission bits inside the private staging directory.
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(mode))
            .map_err(|_| io_error())?;
        file.sync_all().map_err(|_| io_error())?;
        self.sync()
    }
    pub(crate) fn replace(&self, stage: &str, name: &str, absent: bool) -> Result<(), AppError> {
        self.replace_into(stage, self, name, absent)
    }
    pub(crate) fn replace_into(
        &self,
        stage: &str,
        destination: &Self,
        name: &str,
        absent: bool,
    ) -> Result<(), AppError> {
        let stage = cstr(stage.as_ref())?;
        let name = cstr(name.as_ref())?;
        let result = if absent {
            // SAFETY: valid directory descriptor and NUL-terminated components; the OS
            // exclusive-rename flag refuses a destination created after our last check.
            #[cfg(target_os = "linux")]
            {
                unsafe {
                    libc::renameat2(
                        self.file.as_raw_fd(),
                        stage.as_ptr(),
                        destination.file.as_raw_fd(),
                        name.as_ptr(),
                        libc::RENAME_NOREPLACE,
                    )
                }
            }
            #[cfg(target_os = "macos")]
            {
                unsafe {
                    libc::renameatx_np(
                        self.file.as_raw_fd(),
                        stage.as_ptr(),
                        destination.file.as_raw_fd(),
                        name.as_ptr(),
                        libc::RENAME_EXCL,
                    )
                }
            }
        } else {
            // SAFETY: both components and the containing directory descriptor are valid.
            unsafe {
                libc::renameat(
                    self.file.as_raw_fd(),
                    stage.as_ptr(),
                    destination.file.as_raw_fd(),
                    name.as_ptr(),
                )
            }
        };
        if result < 0 {
            return Err(refused(
                "Guarded replacement was refused or is unsupported by this filesystem; existing paths were preserved. Inspect operation history for recovery.",
            ));
        }
        self.sync()?;
        destination.sync()
    }
    pub(crate) fn remove(&self, name: &str) -> Result<(), AppError> {
        let name = cstr(name.as_ref())?;
        // SAFETY: valid directory descriptor and component; flags prohibit directory removal.
        if unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) } < 0 {
            return Err(io_error());
        }
        self.sync()
    }
    pub(crate) fn remove_dir(&self, name: &str) -> Result<(), AppError> {
        let name = cstr(name.as_ref())?;
        // SAFETY: valid fd/component; AT_REMOVEDIR removes only an empty directory, never follows links.
        if unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } < 0 {
            return Err(io_error());
        }
        self.sync()
    }
    pub(crate) fn sync(&self) -> Result<(), AppError> {
        self.file.sync_all().map_err(|_| io_error())
    }
    pub(crate) fn lock(&self) -> Result<File, AppError> {
        let file = match open_at(
            &self.file,
            "lock".as_ref(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        ) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                open_at(&self.file, "lock".as_ref(), libc::O_RDWR, 0).map_err(|_| unsafe_path())?
            }
            Err(_) => return Err(unsafe_path()),
        };
        let m = file.metadata().map_err(|_| io_error())?;
        if !m.is_file() || !owned(&m) || m.nlink() != 1 || m.mode() & 0o777 != 0o600 {
            return Err(unsafe_path());
        }
        no_extended_acl(&file)?;
        file.try_lock().map_err(|_| {
            refused("Another Skillrail mutation holds the journal lock; retry after it finishes.")
        })?;
        Ok(file)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub(crate) identity: Identity,
    pub(crate) bytes: Vec<u8>,
}
impl Snapshot {
    pub(crate) fn group(&self) -> u32 {
        self.identity.gid
    }
    pub(crate) fn mode(&self) -> u32 {
        self.identity.mode & 0o777
    }
}
pub(crate) fn filename(path: &Path) -> Result<&str, AppError> {
    path.file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(unsafe_path)
}

// Conservative ACL boundary: Unix permission bits alone do not describe Darwin ACL grants.
// Refuse ACL-bearing mutation paths instead of dropping ACLs or claiming mode bits prove privacy.
#[cfg(target_os = "macos")]
fn no_extended_acl(file: &File) -> Result<(), AppError> {
    use std::ffi::c_void;
    // Darwin sys/acl.h: acl_t/acl_entry_t are opaque pointers, acl_type_t is an enum.
    unsafe extern "C" {
        fn acl_get_fd_np(fd: i32, acl_type: i32) -> *mut c_void;
        fn acl_get_entry(acl: *mut c_void, entry_id: i32, entry: *mut *mut c_void) -> i32;
        fn acl_free(acl: *mut c_void) -> i32;
    }
    // SAFETY: file fd is live; ACL_TYPE_EXTENDED is the documented Darwin value.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if acl.is_null() {
        // Darwin filesec_get_property reports ENOENT when no ACL property exists.
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
            return Ok(());
        }
        return Err(unsafe_path());
    }
    let mut entry = std::ptr::null_mut();
    // SAFETY: acl is owned from acl_get_fd_np; entry points to a writable pointer slot.
    let has_entry = unsafe { acl_get_entry(acl, 0, &mut entry) } == 0;
    // SAFETY: release exactly the owned ACL allocation, after the borrowed entry is unused.
    unsafe {
        acl_free(acl);
    }
    if has_entry {
        Err(unsafe_path())
    } else {
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn no_extended_acl(file: &File) -> Result<(), AppError> {
    for key in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        // SAFETY: fd and NUL-terminated name are valid; a null, zero-length buffer queries size.
        let size =
            unsafe { libc::fgetxattr(file.as_raw_fd(), key.as_ptr(), std::ptr::null_mut(), 0) };
        if size >= 0 {
            return Err(unsafe_path());
        }
        let error = std::io::Error::last_os_error().raw_os_error();
        if error != Some(libc::ENODATA) && error != Some(libc::ENOTSUP) {
            return Err(unsafe_path());
        }
    }
    Ok(())
}
