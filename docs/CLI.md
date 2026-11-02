# M0 CLI contract

Implemented commands: `version`, `scan [--json]`, `list [--json]`, `tui`.
No subcommand means `tui`. `--help` and `--version` require no application state.
All other planned commands and filters remain unimplemented.

## Paths and isolation

`--home`, `--project`, `--state-dir` are global and work before or after the
subcommand. Explicit flags take precedence. Relative paths resolve against the
process working directory. Home and project must exist and be directories;
paths must be UTF-8. Output context contains absolute, canonical paths, with
symlinks resolved. State is created if necessary after home/project validation.
Only the state directory receives application writes in M0.

| Path | Default |
|---|---|
| Home | `HOME`; supply `--home` if unavailable |
| Project | Current working directory |
| State, Linux | Absolute `XDG_DATA_HOME` + `/skillrail`, otherwise `<home>/.local/share/skillrail` |
| State, macOS | `<home>/Library/Application Support/skillrail` |

An explicit `--home` makes the default state directory relative to that selected
home, ignoring `XDG_DATA_HOME`. An explicit `--state-dir` overrides every state
default. Relative `XDG_DATA_HOME` is ignored. No other environment variables or
configuration files are interpreted; `CODEX_HOME` is not read. Tests and smokes
supply fixture paths explicitly and never reassign HOME or CODEX_HOME.

M0 does not traverse agent directories, parse agent configuration, or execute
agent commands. The selected home/project are context for future adapters.

## Inventory JSON, schema version 1

`scan --json` and `list --json` each scan freshly through the same application
API, atomically replace the cache snapshot, and emit one JSON object plus a
newline on stdout. Successful JSON commands have no stderr diagnostics;
inventory diagnostics are structured inside the object. No progress text or
terminal escapes enter JSON stdout.

```json
{
  "schema_version": 1,
  "context": {
    "home": "/fixture/home",
    "project": "/fixture/project",
    "state_dir": "/fixture/state"
  },
  "scanned_at_unix_ms": 1793624400000,
  "discovery": {"status": "not_implemented", "adapters": []},
  "items": [],
  "occurrences": [],
  "diagnostics": [{
    "code": "discovery_not_implemented",
    "message": "M0 has no discovery adapters; empty inventory does not establish absence of skills or MCP entries."
  }]
}
```

| Field | Type/meaning |
|---|---|
| `schema_version` | Integer, currently 1 |
| `context` | Object of three resolved UTF-8 path strings |
| `scanned_at_unix_ms` | Nonnegative integer milliseconds since Unix epoch |
| `discovery.status` | `not_implemented` in M0 |
| `discovery.adapters` | Empty array in M0; no claim of adapter coverage |
| `items`, `occurrences` | Empty arrays in M0; populated record schemas are deferred |
| `diagnostics` | Array of objects with string `code` and `message` |

For the same resolved context, output is deterministic except the scan timestamp.
Array order is deterministic. There are no item IDs yet because there are no
items. Clients should inspect schema version and diagnostic codes, rather than
parse human messages. Plain-text list/scan shows context and scan time as well
as the discovery limitation. The TUI displays the same scan's time and limitation.
Human-readable paths escape control characters (including embedded newlines);
JSON retains the original path values with normal JSON string escaping.

## Cache

`<state-dir>/index.sqlite3` is disposable. SQLite `PRAGMA user_version` is 1,
independent of the public JSON schema version. A single-row `scan` table stores
the allowlisted JSON snapshot. Schema initialization and snapshot replacement
use one transaction, so a failed replacement does not publish a partial scan.
Reads rebuild rather than trusting stale cached inventory. This is an M0
snapshot cache; query tables can follow when actual inventory exists.

With all Skillrail processes stopped, removing `index.sqlite3` and rerunning
scan/list rebuilds the same normalized inventory. Unsupported schema versions
or corrupt databases fail with a safe error; discard the cache to recover.
Cache symlinks are refused. The cache holds no ownership or undo records.

## Errors and exits

Exit 0 means success (including an empty implemented inventory with an explicit
unimplemented-discovery diagnostic). Operational errors exit 1; argument errors
exit 2. Planned exits 3 (not found) and 4 (conflict/refused/unsupported mutation)
have no M0 command yet.

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
