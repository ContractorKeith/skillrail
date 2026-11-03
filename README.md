# skillrail

A local-first skill and MCP registry for Claude Code and Codex. Planned v1
explains provenance, configured visibility and local drift, with deliberate,
recoverable changes. See [the build plan](docs/PLAN.md) for scope and acceptance.

**Status:** M5 owned snapshot installs from explicit local/Git sources, selected
agent links, guarded preview/apply/undo, CLI/TUI history, shared
inventory queries and read-only stdio MCP. CLI and TUI explain
source identity, installed-version selection, scoped layer observations,
configured status, visibility uncertainty and
full-folder skill drift. MCP values are sanitized before output or caching.
Plugin caches are observed without taking ownership or changing native lifecycle.
[Compatibility](docs/compatibility.md) records the supported schemas, source and
version evidence, uncertainty and pending native/macOS checks. CLI/TUI/MCP
share queries over these sanitized facts. Supported native toggles preserve content
ownership and use a private durable journal separate from the rebuildable index.
Codex skill toggles support user scope; project skill toggles refuse because the
pinned native resolver ignores project skill rules. New snapshots live in
`<home>/.skillrail-sources`; later source edits do not change them.
[Install semantics and limits](docs/CLI.md#owned-snapshot-installation) include
explicit selection, HTTPS-only remote support and conservative source guards.

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
PgUp/PgDn to scroll, and `r` to refresh. `e` enables, `b` disables, and `h`
opens operation history. `i` opens source/selection fields for a snapshot install. Choose an exact occurrence, inspect the preview, then
Enter applies or Escape cancels. In history, `u` previews undo.
`q`, Escape or Ctrl-C quit (Escape cancels an in-progress search). Piped TUI invocation fails
with a diagnostic directing callers to `list --json`.

```bash
skillrail list --kind mcp --agent claude --limit 20 --json
skillrail search review --kind skill --json
skillrail show 'skill-<stable-id>' --json
skillrail drift --json
skillrail disable '<occurrence-id>' --agent codex --scope user --json
skillrail disable '<occurrence-id>' --agent codex --scope user --apply --json
skillrail install /absolute/local/skill --agent codex --scope user --json
skillrail install /absolute/local/repo --git --ref main --skill skills/review --agent claude --scope project --apply --json
skillrail operations --json
skillrail undo 'op-<32-hex-digits>' --apply --json
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
filters, details, timestamps, navigation and resizing. M4 adds guarded mutation,
private permission/alias/ACL, cross-process lock, deterministic interrupted
apply/undo, credential canary and real PTY confirmation/history/undo checks.
M5 adds local/local-Git snapshot manifests, source/collision/security canaries,
real partial-write process termination, install recovery and a PTY install flow.
Linux and macOS GitHub Actions run this exact script. Platform CI results and
independent review must be checked on the PR; local Linux success is not macOS
evidence. Disposable native Claude installation evidence is recorded; broader
agent loading verification remains pending.

Dependencies are limited to Clap (argument/help handling), Serde/JSON (typed
output), rusqlite with bundled SQLite (portable transactional cache), Ratatui
and Crossterm (rendering, terminal events and restoration; its level-triggered
Unix reader preserves simultaneous resize/input readiness), serde-saphyr
(bounded YAML frontmatter), sha2 (stable fingerprints), toml (Codex rule parsing), toml_edit (preserves TOML comments/layout during
config changes), libc (descriptor-relative no-follow I/O and platform-exclusive
rename),
semver (matching native Codex version selection), rmcp (official MCP SDK for
version negotiation, protocol models and bounded stdio transport), Tokio (async
stdio and a limited scan worker; no HTTP transport enabled), and tempfile
(private Git acquisition scratch and isolated fixtures). Python's standard library supplies executable
and PTY smokes without another Rust dependency. Cargo.lock is committed.

## License

MIT — see [LICENSE](LICENSE).
