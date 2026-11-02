# skillrail

A local-first skill and MCP registry for Claude Code and Codex. Planned v1
explains provenance, configured visibility and local drift, with deliberate,
recoverable changes. See [the build plan](docs/PLAN.md) for scope and acceptance.

**Status:** M0 executable harness. The CLI, SQLite cache and minimal terminal
interface work; discovery adapters are not implemented. Empty inventory does
not establish absence of skills or MCP entries. No agent configuration or skill
content is read or modified by M0. M1 supplies skill discovery.

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
Use `q`, Escape or Ctrl-C to quit and `r` to refresh. Piped TUI invocation fails
with a diagnostic directing callers to `list --json`.

[CLI contract](docs/CLI.md) documents defaults, overrides, JSON and errors.

## Development

Run `bash scripts/accept.sh` with stable Rust, rustfmt, Clippy, a C compiler and
Python 3. It checks formatting, strict Clippy, locked tests/build, isolated CLI
scan/list and cache rebuild, plus PTY terminal restoration on quit and refresh
error. Ratatui buffer/event tests exercise rendering, resizing and quit keys.
Linux and macOS GitHub Actions run this exact script. Platform CI results and
independent review must be checked on the PR; local Linux success is not macOS
evidence. No real-agent compatibility checks have been performed in M0.

Dependencies are limited to Clap (argument/help handling), Serde/JSON (typed
output), rusqlite with bundled SQLite (portable transactional cache), Ratatui
and Crossterm (rendering, terminal events and restoration), and dev-only
tempfile (isolated fixtures). Python's standard library supplies executable
and PTY smokes without another Rust dependency. Cargo.lock is committed.

## License

MIT — see [LICENSE](LICENSE).
