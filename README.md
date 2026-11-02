# skillrail

A local-first skill and MCP registry for Claude Code and Codex. Planned v1
explains provenance, configured visibility and local drift, with deliberate,
recoverable changes. See [the build plan](docs/PLAN.md) for scope and acceptance.

**Status:** M3 shared inventory queries and read-only stdio MCP. CLI and TUI explain
source identity, installed-version selection, scoped layer observations,
configured status, visibility uncertainty and
full-folder skill drift. MCP values are sanitized before output or caching.
Plugin caches are observed without taking ownership or changing native lifecycle.
[Compatibility](docs/compatibility.md) records the supported schemas, source and
version evidence, uncertainty and pending native/macOS checks. CLI/TUI/MCP
share queries over these sanitized facts. Mutations remain later milestones.

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
Use `/` to search, Enter for matching details, `k`/`a`/`s` to cycle kind/agent/scope,
`d` to toggle drifted/unknown skills, `x` to clear filters, arrows to select,
PgUp/PgDn to scroll, and `r` to refresh.
`q`, Escape or Ctrl-C quit (Escape cancels an in-progress search). Piped TUI invocation fails
with a diagnostic directing callers to `list --json`.

```bash
skillrail list --kind mcp --agent claude --limit 20 --json
skillrail search review --kind skill --json
skillrail show 'skill-<stable-id>' --json
skillrail drift --json
skillrail --project /absolute/project mcp
```

Queries scan freshly and return bounded pages with an inventory revision;
TUI search uses its timestamped snapshot until manual refresh. MCP exposes only
list/search/details/visibility/drift. [CLI contract](docs/CLI.md) documents exact
JSON, pagination, errors, protocol limits and Claude Code/Codex setup examples.

## Development

Run `bash scripts/accept.sh` with stable Rust, rustfmt, Clippy, a C compiler and
Python 3. It checks formatting, strict Clippy, locked tests/build, isolated CLI
scan/list and cache rebuild, plus PTY terminal restoration on quit and refresh
error, search-to-detail, refresh and resize. The official Rust MCP SDK client
initializes, lists and calls every read tool; raw stdio probes cover negotiation,
malformed/oversized inputs and EOF. Ratatui buffer/event tests cover queries,
filters, details, timestamps, navigation and resizing.
Linux and macOS GitHub Actions run this exact script. Platform CI results and
independent review must be checked on the PR; local Linux success is not macOS
evidence. Disposable native Claude installation evidence is recorded; broader
agent loading verification remains pending.

Dependencies are limited to Clap (argument/help handling), Serde/JSON (typed
output), rusqlite with bundled SQLite (portable transactional cache), Ratatui
and Crossterm (rendering, terminal events and restoration), serde-saphyr
(bounded YAML frontmatter), sha2 (stable fingerprints), toml (Codex rule parsing),
semver (matching native Codex version selection), rmcp (official MCP SDK for
version negotiation, protocol models and bounded stdio transport), Tokio (async
stdio and a limited scan worker; no HTTP transport enabled), and dev-only tempfile
(isolated fixtures). Python's standard library supplies executable
and PTY smokes without another Rust dependency. Cargo.lock is committed.

## License

MIT — see [LICENSE](LICENSE).
