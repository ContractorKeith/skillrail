# CLI contract (M3)

Implemented commands:

```text
skillrail [tui]
skillrail version
skillrail scan [--json]
skillrail list [filters] [--json]
skillrail search <query> [filters] [--json]
skillrail show <id> [--json]
skillrail drift [filters] [--json]
skillrail mcp
```

No subcommand means `tui`. `--help` and `--version` require no application state.
Filters: `--kind skill|mcp`, `--agent claude|codex`,
`--scope user|project|local|unknown`, `--limit 1..200` (default 100),
`--offset 0..1000000` (default 0), `--revision <64-lowercase-hex>`.
Global paths below apply to every command. No mutation commands are implemented.

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

M2 reads standalone skill folders, MCP definitions and supported plugin cache
candidates with bounded local provenance/config evidence.
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
M1/M2 fill the item/occurrence arrays with typed records:

Incomplete discovery diagnostics carry a structured `location` with `path`,
`agent`, `scope`, and a safe failure `category`. CLI/TUI render this context with
terminal controls escaped; JSON preserves the original path. Generic diagnostics
without a specific location omit this field. Raw operating-system error strings
and configuration contents are never included.

- Item: `id`, `kind` (`skill` or `mcp`), `name`, optional `description`, `source`
  (`identity`, `evidence`, optional `repository`/`subpath`, `owner`),
  `name_collision`, and `drift` (`single_source`, `equal`, `drifted`, `unknown`).
- Occurrence: stable `id`, `item_id`, `name`, `agent` (`claude`/`codex`),
  `scope` (`user`/`project`/`local`/`unknown`), `original_path`, nullable `canonical_path`,
  `invocations` (sorted names and per-invocation `shadowed_in_default_layout`),
  `visibility` (`status` and `reasons`), `fingerprint` (`algorithm`, nullable
  `sha256`, `diagnostics`), and metadata/target `diagnostics`.
- Discovery: `status: inventory_partial`, adapters
  `claude_standalone_v1`, `codex_standalone_v1`, `claude_mcp_plugins_v1` and
  `codex_mcp_plugins_v1`.
- Top-level diagnostics: safe `code`/`message` pairs. `coverage_partial` always
  states excluded coverage. Root failures and configuration errors add explicit
  diagnostics. Per-occurrence diagnostics must also be inspected.

`scan` and `list` both refresh. Results are sorted by stable IDs; timestamps
are the only expected difference for unchanged supported fixture trees. Deleting
the cache and rescanning preserves identities and facts. Unknown states do not
cause an operational error or imply absence. Source and original paths can
contain controls: JSON uses ordinary escaping, while human output and TUI
escape terminal controls and bidirectional formatting controls.

Plain scan prints context, scan time and every item's explanations. Plain queries
print the selected page; multi-page output adds offset, revision and next offset.
The TUI uses the same query matcher and human explanation formatter: `/` edits
search, Enter leaves search for the matching details, Backspace edits, Escape
cancels search, `k` cycles all/skill/mcp, `a` cycles all/claude/codex, `s` cycles
all/user/project/local/unknown, `d` toggles the same drifted/unknown skill subset
as CLI/MCP drift, and `x` clears search/filters including drift. Arrows select
matching items, PgUp/PgDn scroll details, `r` rescans, q/Esc/Ctrl-C quit outside
search (Ctrl-C also quits during search). Typed q/r/filter letters are search
text while editing. Search/filter changes use the displayed snapshot, without
rescanning; Last scan changes only after a successful startup/manual scan.
Resizing redraws the same snapshot.
Claude linked-worktree fallback entries retain main-checkout original/canonical
paths and an explicit visibility reason. Unsafe or unreadable fallback metadata
adds `worktree_fallback_unknown`; Codex never receives this fallback. Invocation
precedence applies to the matching command name only, independently of source
identity. Diagnostics remain available below the selected item's details. A small
terminal can scroll the entire view. Mutations remain later work.

## MCP and plugin records

The schema-version-1 envelope and cache version remain unchanged. M2 extends
the tagged kinds, scopes and occurrence fields. Consumers must handle new enum
values and additive fields. M1 skill IDs, fingerprints and visibility facts are
unchanged; the discovery coverage label and human headings now describe mixed
inventory.

Every occurrence adds `origin`, nullable `mcp`, `capability` and `observations`:

- `origin.kind` is `standalone` or `plugin`. Plugin origins retain `root`,
  `manifest`, validated `namespace`, and `configured_status` summarizing known
  activation observations (`enabled`, `disabled`, `invalid`, `unknown`).
- Plugin `origin.installation` has `status` (`selected`, `residual`, `unknown`),
  optional version and evidence path. Supported Claude v2 install records give
  selected user/project/local scope; unrelated-project records do not apply.
  Codex store-version selection follows the pinned native algorithm, while
  installation scope stays unknown. A cache version and activation rule are
  separate facts; neither proves runtime visibility.
- `observations` is a list of source-path/scope/setting/status/fixed-reason
  records. Known user facts survive unrelated project configuration. Conflicts
  summarize as unknown without dropping either observation. Malformed supported
  settings are invalid; unsupported policy schemas are unknown. Claude persisted
  project opt-outs and MCP-JSON rejection/approval lists remain separate settings.
- `mcp` retains configured status and transport (`stdio`, `http`, `sse`,
  `websocket`, `unknown`). Supported definitions default on; known supported
  disables change the status to disabled. Unsupported declarations and truncated
  plugin loading make final status unknown. This describes local configuration,
  not complete native validation, effective trust/precedence or health.
- `capability` retains `toggle:false` and the read-only/native-ownership reason.
  Supported mutations belong to M4. Runtime `visibility` remains unknown.

Root Claude plugin `SKILL.md` fallback retains native ownership, with one
occurrence per applicable scope. Unrelated plugin folders are not enumerated;
the fallback's full-folder fingerprint is explicitly unknown. All installation
and layer facts appear in the shared human formatter/TUI and sanitized cache.
See compatibility for exact evidence, cumulative loader budgets, conservative
duplicate-key rejection and unresolved native behaviors.

MCP item identity includes agent, scope, original configuration path, selector
and server key. Names, command equality and URL equality never join servers.
MCP records have null descriptions and fingerprint digests, with
`algorithm:not_applicable` and unknown drift. Plugin skill
identity uses plugin root and canonical skill target; matching names/content in
other caches do not prove shared provenance. Plugin skill invocations are empty
because runtime namespace/selection is unresolved. Ownership is `native_plugin`
or `external_configuration` for MCP, while M1 skill ownership is retained.

Only labels, paths, enums, IDs and fixed explanations cross the configuration
parser seam. Commands, arguments, environment names/values, headers, URL parts,
authentication objects, unknown field values and parse-error context never enter
the public record, terminal view or SQLite. Unsafe/unsupported server labels
are replaced with `unsupported-name`; their source keys contribute only to a
one-way ID. Diagnostics identify the configuration file, never raw values.
Original/canonical configuration paths identify the observed filesystem file.
All new terminal details use the same escaping as M1.

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
exit 2. `show` with an unknown stable item ID exits 3 (`item_not_found`).
Exit 4 remains reserved for future refused mutations.

Operational errors produce no stdout. With any inventory command using `--json`, stderr
contains one object of this form:

```json
{"schema_version":1,"error":{"code":"invalid_state_dir","message":"Cannot create or access the state directory."}}
```

Without `--json`, stderr contains `code: message`. Codes include `invalid_home`,
`invalid_project`, `invalid_state_dir`, `invalid_path`, `index_unavailable`,
`index_version_unsupported`, `clock_unavailable`, `output_unavailable`,
`terminal_required` and `terminal_unavailable`. Query validation errors use `invalid_arguments`
(exit 2, JSON stderr when `--json` is present); Clap parse errors retain text
usage. `inventory_changed` and `response_too_large` exit 1. `mcp_unavailable`
means initialization/transport failure (stderr only). Raw OS/SQLite errors and raw
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

## Shared queries and pagination

`list`, `search`, `show`, `drift` and all MCP read tools scan freshly using
`Application::query`. TUI matching uses `Query::indices` over its last scan.
No interface repeats filesystem discovery. `scan` retains the complete M0–M2
snapshot envelope, without pagination; `list` now adds `page` and defaults to
100 items. Schema/cache versions remain 1; existing item and occurrence IDs
are unchanged. Consumers must accept additive fields.

Search uses a case-insensitive Unicode-lowercase substring of item name,
description or stable ID. Query text must be nonblank and at most 256 UTF-8
bytes; ID arguments must be 1–256 UTF-8 bytes. Valid but unknown IDs are not
found; malformed query arguments fail before scanning. Filters combine with
AND. Agent and scope must match the same occurrence. A matching item retains
**all** its occurrences so provenance and other-agent visibility are not lost.
`drift` returns skills with `drifted` or `unknown` drift, including unresolved
fingerprints, excluding MCP records and equal/single-source skills. Unknown
trust/session state remains unknown in details and visibility results.

Query results retain the inventory envelope with selected `items`/`occurrences`
and complete scan diagnostics/context, plus:

```json
{"page":{"revision":"<64 hex characters>","offset":0,"limit":100,"total":123,"next_offset":100}}
```

Items and occurrences sort lexicographically by their existing stable IDs.
`total` counts matching items before pagination; empty/out-of-range pages are
successful and `next_offset` is null. Offset > 0 requires the previous page's
revision. The revision hashes all sanitized inventory facts/context except scan
time, independently of query/limit/offset. Keep the same filters/search while
paging. Any changed fact (including diagnostics) returns `inventory_changed`;
restart at offset 0 without revision. Unchanged scans keep revision even when
time changes. Pagination never silently combines different observed inventories;
scans are observations of files and do not promise an atomic filesystem snapshot.

The encoded query JSON is capped at 1 MiB, including diagnostics and all selected
occurrences. Serialization stops at that limit and returns `response_too_large`
without partial query stdout. Reduce limit/narrow filters; a single oversized
item or diagnostic set also fails explicitly, without truncating explanations.
This limit applies to CLI queries and MCP tool content; MCP JSON-RPC escaping
adds framing overhead. `scan` retains its complete snapshot contract.

```bash
skillrail list --kind mcp --agent claude --limit 20 --json
skillrail search 'review' --kind skill --scope user --json
skillrail show 'skill-<stable-id>' --json
skillrail drift --agent codex --json
skillrail list --limit 20 --offset 20 --revision '<previous-page-revision>' --json
```

## Read-only MCP stdio

Start `skillrail --project /absolute/project mcp`; global home/state overrides
are supported for fixtures. Stdout is newline-delimited JSON-RPC only. No logs,
raw config, command/argument/env/header/URL values, skill bodies, bundled files,
resources, or write tools are served. Only the rebuildable state cache is written.
Configured servers/skills are never started and no network discovery/probe runs.
No tracing subscriber is installed, so SDK parser error context is not logged.

Initialize negotiates `2024-11-05`, `2025-03-26`, `2025-06-18`, or `2025-11-25`;
an unsupported client proposal receives a supported version, which the client
must accept or disconnect. Send `notifications/initialized` after the response.
The capability set is tools only. `tools/list` returns all five definitions in
fixed order, with input schemas and read-only/idempotent/closed-world annotations;
there is no next cursor, and supplying a cursor is invalid.

| Tool | Arguments | Result |
|---|---|---|
| `list_inventory` | Optional kind, agent, scope, limit, offset, revision | Shared paginated inventory |
| `search_inventory` | Required query plus list arguments | Shared paginated matches |
| `get_item` | Required id only | Shared envelope with one item and all occurrences |
| `explain_visibility` | Required id only | Same item envelope, including complete visibility reasons/observations |
| `list_drift` | List arguments | Shared envelope for drifted/unknown skills |

All tool argument objects reject unknown fields; types/ranges match CLI queries.
Missing/invalid arguments, unknown tool names and invalid tools/list cursors return
JSON-RPC `-32602` with fixed safe text. Unavailable methods/resources use `-32601`.
SDK envelope-decoding exception: a non-string tool `name` or non-null, non-object
`arguments` in outer `tools/call` params returns `-32601` before the tool
handler runs. The pinned SDK dispatches these as unrecognized requests; this
does not mean the named tool is unavailable. Well-formed argument objects
reach Skillrail validation and use `-32602` for invalid fields/values.
An outer `arguments:null` is treated as omitted: tools with optional arguments
accept it, while tools requiring an ID or query return `-32602`.
Unknown IDs and operational query failures return normal MCP tool results with
`isError:true` and text JSON `{schema_version:1,error:{code,message}}`. Success
uses `isError:false` and one `content` text block containing the exact shared
query JSON. There is no structuredContent duplicate or skill-body resource.

Transport lines are limited to 64 KiB (including newline). Following rmcp 3.8.0,
malformed JSON syntax and oversized lines are discarded, with recovery at the
next newline; structurally invalid JSON-RPC receives sanitized `-32600` with null
ID. Unknown notifications are ignored. Keep stdin open until the response has
arrived; close stdin to disconnect. Incomplete trailing lines are dropped.
Disconnect after initialization exits cleanly with no stdout trailer or stderr.
Initialization has a 30-second deadline. Each scan has a 30-second response
deadline, runs off the protocol runtime, and holds the single scan slot until
completion even after timeout/cancellation. Concurrent scans receive a fixed
`-32603` busy/retry error rather than queuing more filesystem work. Disconnect
observes stdin EOF before the SDK response drain and bounds runtime shutdown
to 100 ms; a blocked filesystem call cannot hold process exit. Normal filesystem/cache errors remain safe tool errors.

Example client configuration (edit manually; Skillrail does not install it):

Claude Code `.mcp.json`:

```json
{"mcpServers":{"skillrail":{"command":"/absolute/path/skillrail","args":["--project","/absolute/project","mcp"]}}}
```

Codex `config.toml`:

```toml
[mcp_servers.skillrail]
command = "/absolute/path/skillrail"
args = ["--project", "/absolute/project", "mcp"]
```

Use actual absolute paths. These examples select project context explicitly;
native approval/trust remains the client's responsibility. They make no claim
that any current session loaded the server.
