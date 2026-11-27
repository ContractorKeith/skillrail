# 0002: Guarded filesystem mutation and the recovery journal

Status: accepted (recorded from code comments, 2026-11-27)

## Context
Skillrail edits real agent configs and publishes owned skill trees. A
substituted path, a crash partway through, or a concurrent process must never
silently corrupt a file or strand recovery.

## Decisions
### Descriptor-relative I/O (`src/guarded_fs.rs`)
- All mutation I/O is descriptor-relative (`openat`, `mkdirat`, `fstatat`,
  `readlinkat`, `symlinkat`, `renameat`, `unlinkat`). Final components are
  never followed, and recovery bytes are never read through unchecked aliases.
- Ancestors must be owned by the effective user and must not be group- or
  world-writable. The one exception is a root-owned sticky temp root, such as
  `/tmp`. Everything below it must still be trusted.
- `openat`'s variadic mode is promoted to `c_uint` because Darwin's `mode_t`
  is only `u16`.
- New publication uses exclusive rename (`RENAME_NOREPLACE` on Linux), so a
  destination created after our last check is refused, not overwritten.
  `symlinkat` never overwrites. `AT_REMOVEDIR` removes only empty directories.
- Restored files keep their original group and permission bits. They're set
  while the file is still inside the private staging directory.
- macOS ACLs: mode bits don't describe Darwin ACL grants, so a path that
  carries an extended ACL is refused instead of dropping the ACL or claiming
  privacy. `filesec` reports `ENOENT` when no ACL exists. The Darwin ACL
  functions are declared locally because `libc` doesn't expose them.
- `Snapshot::reservation` is an upper bound used only to size serialization.
  It's never a filesystem pre- or postcondition.

### Plans, locks and the journal (`src/mutation.rs`, `src/mutation/journal_record.rs`)
- `Plan` holds private snapshots and has no `Debug` or `Serialize`
  implementation. Public views can't serialize recovery content. The TUI keeps
  the actual private plan between preview and confirmation.
- Toggles plan a batch of recoverable changes to several files. Skillrail does
  not claim a filesystem transaction.
- Stale selections are rejected before a journal or lock is created. The same
  checks are repeated under the application lock.
- The lock is unlocked explicitly, which also releases descriptors briefly
  inherited by an unrelated fork. Replacing or aliasing the lock file after
  acquisition is detected, so cooperating processes can't split.
- Journal format v2 stores each before/after byte sequence once, including
  during undo. `admit` reserves the largest possible serialized state before
  anything is recorded or published. Bytes and parent chains never grow,
  future identities use maximum-width numbers, every file can have both a
  published replacement and an undo stage, and undo is planned once.
- Restoration validates the complete change set before touching any file, and
  plans once while keeping every staged or restored inode across retries.
- An unrecorded write can stop at any prefix, but a recorded stage must match
  its durable snapshot exactly. A recorded stage can't disappear without an
  observed publication or edit.
- A committed stage inode is reused, never recreated. Recreating it before new
  evidence is saved can strand recovery if the process dies during the
  replacement's fsync. A stage whose identity was never committed can be
  rebuilt.
- Path binding is revalidated after publication.
- `apply_with_checkpoint` is a deterministic seam for testing interruptions
  and races. A checkpoint error leaves the same durable state as process
  death. There's no environment flag or production bypass.

### Owned installs (`src/mutation/install*.rs`)
- Install, update and uninstall share the journal, lock and plan lifecycle.
  Originals are retained privately, and replacements are published with the
  same guards as toggles.
- Renaming a stage to its snapshot changes only the basename, never
  descriptor identities.
- Plugin skills join after standalone root completeness is computed, because
  their incomplete fingerprints can hide references to an owned snapshot.
- An applied install requires complete postconditions. A retained backup must
  not let an older install be marked undone while it still owns backups.
- During an interrupted apply or undo, a link may be absent. It may never
  point anywhere else.
- Stages, backups, replacements and the published link are reserved up front,
  so Skillrail's own lifecycle can't push a complete root over the scan budget
  during undo. Future inode metadata and directory chains are bounded at the
  start, and bytes appear only in entries.
- Before a new mutation, the original links must exist with their exact
  durable identities. Once an old link is restored, its new counterpart is
  already gone, so cleanup removes only remaining new paths.
- Update pins selection to the manifest, even if the source later gains
  another skill. It keeps the declared HTTPS endpoint and normalizes only
  local paths.
- A snapshot's identity is a stable sorted representation of paths, bytes and
  executable bits. Every source and journal path admits the same selected-tree
  shape (`validate_entries`).

### Config patches (`src/config_patch.rs`)
- A JSON patch replaces one value span or inserts one key, and every other
  byte stays as it was.
- TOML patches support both native array shapes without reserializing
  unrelated fields. `toml_edit` emits LF, so CRLF is restored only when
  reparsing proves the values are unchanged. Files with mixed line endings
  refuse.
