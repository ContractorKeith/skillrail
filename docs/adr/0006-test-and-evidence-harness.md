# 0006: Test and evidence harness

Status: accepted (recorded from code comments, 2026-11-27)

## Isolation
- Tests and evidence scripts never touch real agent configs (AGENTS.md).
  Passing `--home` explicitly also moves the default state directory away
  from XDG.
- Claude evidence uses Claude's documented config-directory override to keep
  native writes isolated. `HOME` and `CODEX_HOME` are inherited, never
  overwritten.
- Each fixture gets its own Git repository boundary, so Skillrail's ancestor
  discovery can't reach the author's worktree. Git runs in tests only to
  create disposable fixtures.
- Build caches can have writable ancestors, so mutation fixtures live under a
  resolved private temp directory that ADR 0002 accepts as trusted.
- The Homebrew validation test clears `HOMEBREW_NO_AUTOREMOVE` and
  `HOMEBREW_NO_INSTALL_CLEANUP`, so an inherited setting can't mask a missing
  script guard.
- Native evidence compiles the exact pinned upstream function with small typed
  fixture collaborators: native code supplies the policy, fixtures supply only
  data. A negative control proves the fixture notices when the guard is
  removed.

## PTY driving
- The TUI uses crossterm's `use-dev-tty` level-triggered Unix reader. It keeps
  input readiness when a resize is returned first in the same batch.
  `scripts/pty-readiness-smoke.py` forces that case for real.
- The pinned Ratatui/Crossterm emits Hide (`ESC[?25l`) after each complete
  draw. Smokes treat that as the frame boundary and never match leftover text
  under a partly drawn screen. A PTY read can end between a header and its
  body.
- The parent keeps draining the PTY while waiting for exit, because redraw and
  restore writes can fill it (especially on macOS) and block the child.
- Darwin sets `PENDIN` when restoring canonical mode, even when `tcsetattr`
  gets the exact saved attributes. It's transient pending-input state, and
  only that bit is excused on Darwin (`scripts/test_terminal_attributes.py`).

## Filesystem realism
- Case-insensitive filesystems: tests check real directory entries and types,
  not alias lookups, and use two renames to change case while keeping the
  inode.
- Identity-conflict tests hold the first inode open, so the filesystem can't
  recycle its number during the second undo.
