# skillrail

A local-first skill and MCP registry for Claude Code and Codex. Planned v1
explains provenance, configured visibility and local drift, with deliberate,
recoverable changes. See [the build plan](docs/PLAN.md) for scope and acceptance.

**Status:** M1 standalone skill inventory. CLI and TUI explain local source
identity, aliases, name collisions, configured visibility uncertainty and
full-folder drift. Scans read supported user/project skill locations and write
only the disposable cache. [Compatibility](docs/compatibility.md) records
sources, checked CLI versions, boundaries and pending real-agent verification.
Plugins, MCP queries and mutations remain later milestones.

## Run

Build with stable Rust and a C compiler (SQLite is bundled):

```bash
cargo build --locked
cargo run --locked -- version
cargo run --locked -- --help
cargo run --locked -- --home /path/to/home --project /path/to/project --state-dir /path/to/state scan --json
cargo run --locked -- --home /path/to/home --project /path/to/project --state-dir /path/to/state list --json
```

`skillrail` or `skillrail tui` opens the TUI when stdin and stdout are terminals.
Use arrows to select, PgUp/PgDn to scroll details, `r` to refresh, and
`q`, Escape or Ctrl-C to quit. Piped TUI invocation fails
with a diagnostic directing callers to `list --json`.

[CLI contract](docs/CLI.md) documents defaults, overrides, JSON and errors.

## Development

Run `bash scripts/accept.sh` with stable Rust, rustfmt, Clippy, a C compiler and
Python 3. It checks formatting, strict Clippy, locked tests/build, isolated CLI
scan/list and cache rebuild, plus PTY terminal restoration on quit and refresh
error. Ratatui buffer/event tests exercise rendering, resizing and quit keys.
Linux and macOS GitHub Actions run this exact script. Platform CI results and
independent review must be checked on the PR; local Linux success is not macOS
evidence. Real-agent load verification remains pending; version-only evidence
is recorded.

Dependencies are limited to Clap (argument/help handling), Serde/JSON (typed
output), rusqlite with bundled SQLite (portable transactional cache), Ratatui
and Crossterm (rendering, terminal events and restoration), serde-saphyr
(bounded YAML frontmatter), sha2 (stable fingerprints), toml (Codex rule parsing),
and dev-only tempfile (isolated fixtures). Python's standard library supplies executable
and PTY smokes without another Rust dependency. Cargo.lock is committed.

## License

MIT — see [LICENSE](LICENSE).
