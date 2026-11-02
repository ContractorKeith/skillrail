# CLI contract (M1)

Implemented commands: `version`, `scan [--json]`, `list [--json]`, `tui`.
No subcommand means `tui`. `--help` and `--version` require no application state.
All other planned commands and filters remain unimplemented.

## Paths and isolation

`--home`, `--project`, `--state-dir` are global and work before or after the
subcommand. Explicit flags take precedence. Relative paths resolve against the
process working directory. Home and project must exist and be directories;
paths must be UTF-8. Output context contains absolute, canonical paths, with
symlinks resolved. State is created if necessary after home/project validation.
Only the state directory receives application writes.

| Path | Default |
|---|---|
| Home | `HOME`; supply `--home` if unavailable |
| Project | Current working directory |
| State, Linux | Absolute `XDG_DATA_HOME` + `/skillrail`, otherwise `<home>/.local/share/skillrail` |
| State, macOS | `<home>/Library/Application Support/skillrail` |

An explicit `--home` makes the default state directory relative to that selected
home, ignoring `XDG_DATA_HOME`. An explicit `--state-dir` overrides every state
default. Relative `XDG_DATA_HOME` is ignored. No other environment variables affect path resolution; `CODEX_HOME` is not read. Tests and smokes
supply fixture paths explicitly and never reassign HOME or CODEX_HOME.

M1 reads standalone skill folders and bounded local provenance/config evidence.
See [compatibility](compatibility.md) for supported roots and restrictions.
Agent commands and skill content are never executed during scans.

## Inventory JSON, schema version 1

`scan --json` and `list --json` each scan freshly through the same application
API, atomically replace the cache snapshot, and emit one JSON object plus a
newline on stdout. Successful JSON commands have no stderr diagnostics;
inventory diagnostics are structured inside the object. No progress text or
terminal escapes enter JSON stdout.

The version-1 object retains `schema_version`, resolved `context`,
`scanned_at_unix_ms`, `discovery`, `items`, `occurrences`, and `diagnostics`.
M1 fills the previously empty item/occurrence arrays with typed records:

- Item: `id`, `kind` (`skill`), `name`, optional `description`, `source`
  (`identity`, `evidence`, optional `repository`/`subpath`, `owner`),
  `name_collision`, and `drift` (`single_source`, `equal`, `drifted`, `unknown`).
- Occurrence: stable `id`, `item_id`, `name`, `agent` (`claude`/`codex`),
  `scope` (`user`/`project`), `original_path`, nullable `canonical_path`,
  `invocations` (sorted names and per-invocation `shadowed_in_default_layout`),
  `visibility` (`status` and `reasons`), `fingerprint` (`algorithm`, nullable
  `sha256`, `diagnostics`), and metadata/target `diagnostics`.
- Discovery: `status: standalone_skills_partial`, adapters
  `claude_standalone_v1` and `codex_standalone_v1`.
- Top-level diagnostics: safe `code`/`message` pairs. `coverage_partial` always
  states excluded coverage. Root failures and configuration errors add explicit
  diagnostics. Per-occurrence diagnostics must also be inspected.

`scan` and `list` both refresh. Results are sorted by stable IDs; timestamps
are the only expected difference for unchanged supported fixture trees. Deleting
the cache and rescanning preserves identities and facts. Unknown states do not
cause an operational error or imply absence. Source and original paths can
contain controls: JSON uses ordinary escaping, while human output and TUI
escape terminal controls and bidirectional formatting controls.

Plain list/scan prints context, scan time and every item's explanations.
The TUI uses exactly the same inventory and human explanation formatter: arrows
select items, PgUp/PgDn scroll details, `r` rescans, q/Esc/Ctrl-C quit.
Claude linked-worktree fallback entries retain main-checkout original/canonical
paths and an explicit visibility reason. Unsafe or unreadable fallback metadata
adds `worktree_fallback_unknown`; Codex never receives this fallback. Invocation
precedence applies to the matching command name only, independently of source
identity. Diagnostics remain available below the selected item's details. A small
terminal can scroll the entire view. Search/filters, MCP and mutations remain later work.

## Cache

`<state-dir>/index.sqlite3` is disposable. SQLite `PRAGMA user_version` is 1,
independent of the public JSON schema version. A single-row `scan` table stores
the allowlisted JSON snapshot. Schema initialization and snapshot replacement
use one transaction, so a failed replacement does not publish a partial scan.
Reads rebuild rather than trusting stale cached inventory. The snapshot stores
the allowlisted typed inventory; normalized query tables can follow when query
interfaces require them.

With all Skillrail processes stopped, removing `index.sqlite3` and rerunning
scan/list rebuilds the same normalized inventory. Unsupported schema versions
or corrupt databases fail with a safe error; discard the cache to recover.
Cache symlinks are refused. The cache holds no ownership or undo records.
Snapshot replacement verifies the saved row before committing; altered schema
objects that discard or rewrite it cause rollback. The state directory must be
trusted: symlink refusal does not prevent a deliberately planted hard-link alias
from naming another SQLite database. Do not share state directories with
untrusted writers or hard-link an existing database into them.

## Errors and exits

Exit 0 means a completed scan/cache replacement, including explicit unknown or
partial inventory states. Operational errors exit 1; argument errors
exit 2. Planned exits 3 (not found) and 4 (conflict/refused/unsupported mutation)
have no implemented command yet.

Operational errors produce no stdout. With `scan --json` or `list --json`, stderr
contains one object of this form:

```json
{"schema_version":1,"error":{"code":"invalid_state_dir","message":"Cannot create or access the state directory."}}
```

Without `--json`, stderr contains `code: message`. Codes include `invalid_home`,
`invalid_project`, `invalid_state_dir`, `invalid_path`, `index_unavailable`,
`index_version_unsupported`, `clock_unavailable`, `output_unavailable`,
`terminal_required` and `terminal_unavailable`. Raw OS/SQLite errors and raw
configuration data are never included. Argument errors use Clap's text usage
on stderr, including when a malformed command mentions `--json`; help and
version use stdout and exit 0.
Failed stdout writes, including closed pipes, exit 1 with `output_unavailable`
rather than panicking. If stderr is also unavailable, the exit code is retained.
Output already accepted by a failing sink cannot be retracted.

The TUI requires terminal stdin/stdout and refuses piped execution before any
state creation. Normal quit, Ctrl-C key handling, errors and Rust unwinding
attempt terminal restoration; Ratatui also installs its panic restoration hook.
Uncatchable process termination cannot run cleanup. PTY smokes verify restored
terminal attributes and alternate-screen exit on quit and a refresh error.
