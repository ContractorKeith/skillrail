# CLI contract (M5)

Implemented commands:

```text
skillrail [tui]
skillrail version
skillrail scan [--json]
skillrail list [filters] [--json]
skillrail search <query> [filters] [--json]
skillrail show <id> [--json]
skillrail drift [filters] [--json]
skillrail enable <item-or-occurrence-id> --agent claude|codex --scope user|project|local [--expect <token>] [--apply] [--json]
skillrail disable <item-or-occurrence-id> --agent claude|codex --scope user|project|local [--expect <token>] [--apply] [--json]
skillrail install <source> --agent claude|codex [--agent claude|codex] --scope user|project [--git] [--ref <ref>] [--skill <subpath>] [--expect <token>] [--apply] [--json]
skillrail operations [--json]
skillrail undo <operation-id> [--apply] [--json]
skillrail mcp
```

No subcommand means `tui`. `--help` and `--version` require no application state.
Filters: `--kind skill|mcp`, `--agent claude|codex`,
`--scope user|project|local|unknown`, `--limit 1..200` (default 100),
`--offset 0..1000000` (default 0), `--revision <64-lowercase-hex>`.
Global paths below apply to every command. Update/uninstall remain M6.

## Paths and isolation

`--home`, `--project`, `--state-dir` are global and work before or after the
subcommand. Explicit flags take precedence. Relative paths resolve against the
process working directory. Home and project must exist and be directories;
paths must be UTF-8. Output context contains absolute, canonical paths, with
symlinks resolved. State is created if necessary after home/project validation.
Reads write only the rebuildable cache. Explicit applied mutations also write the previewed configuration paths and private sibling staging directories.

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
terminal can scroll the entire view. See the M4 mutation controls below.

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
- `capability` reports native toggle availability plus the read-only inventory/native-ownership explanation.
  M4 advertises a native toggle preview for standalone entries; each preview still checks schema, selectors and filesystem safety. Plugin capabilities remain refused. Runtime `visibility` remains unknown.

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
Mutation conflicts, unsafe paths, unsupported native changes and busy mutation locks exit 4.

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
terminal attributes and alternate-screen exit on quit and a refresh error,
including backpressured redraws with output drained during the exit wait.
On Darwin these comparisons exclude only `PENDIN`, transient pending-input state
that the OS can set when restoring canonical mode; all other attributes must match.

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
next newline; structurally invalid JSON-RPC receives sanitized `-32600` without
a correlated ID (the SDK omits the `id` field). Unknown notifications are ignored.
Keep stdin open until the response has arrived; close stdin to disconnect. Incomplete trailing lines are dropped.
Disconnect after initialization exits cleanly with no stdout trailer or stderr.
An adapter keeps the SDK receive future in one persistent task so ordinary
service polling cannot cancel a parser error after its input line was consumed.
It forwards requests through a capacity-one channel; at most one further parsed
request waits in the reader. Normal replies and parser errors use SDK framing
and one shared writer lock held through frame flush. Close/drop stops the reader;
write failures release the shared lock. No parser or raw-payload error path is
reimplemented. `python3 scripts/mcp-smoke.py --repeat 20` runs a bounded 100-session
stress pass, including 1,600 pipelined malformed requests; the default gate runs
five sessions with 80 such requests. Client timeouts are unchanged.

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

## M4 guarded visibility changes

Both `--agent` and occurrence `--scope` are required. An item ID works only when
one occurrence matches; otherwise use the exact occurrence ID from `scan`/`show`.
No interactive CLI picker or inferred agent/scope exists. `enable`/`disable`
preview by default. `--apply` computes and revalidates a plan, then writes it.
To bind separate CLI invocations, pass the earlier preview's `token` with
`--expect`; any changed target bytes/identity/permissions or observed selection
facts refuse the write. The token includes the complete sanitized selection revision,
including canonical source and fingerprint facts, plus exact consulted config bytes
and target preconditions. TUI confirmation retains the actual private plan.
A CLI apply without `--expect` authorizes its freshly computed plan.

Mutation JSON has `schema_version:1`, `status` (`preview`, `applied`, `undone`,
`unchanged`), `action` (`toggle` or `undo`), a random stable `operation_id`,
opaque `token`, exact normalized `targets` (occurrence ID, agent, scope,
requested enablement), `files`, safe file `changes` (create/replace/restore/remove),
and fixed explanatory `notes`. It contains no raw configuration, field values,
commands, arguments, credentials or backups. Undo targets describe the original
operation; `action:undo` and each change explicitly indicate restoration.
A no-op creates no operation record. Its provisional preview ID is not history.
Plain output is the same safe object, indented, with terminal controls escaped.

Supported mechanisms and constraints are pinned in
[compatibility](compatibility.md#m4-native-toggle-evidence-and-support):

- Claude standalone skills: `skillOverrides.<name>` = `off`/`on` in user
  `.claude/settings.json` for a user occurrence, or selected-project
  `.claude/settings.json` for a project occurrence. Project enable writes explicit
  `on` when user/ancestor settings have a restrictive listing state; absence in
  the project file is inheritance. A known restrictive local override refuses.
  No skill content/link moves.
- Codex standalone skills: append a canonical SKILL.md path rule to
  `skills.config` in user `.codex/config.toml` for user occurrences. The pinned
  native resolver reads only User and SessionFlags layers, ignoring project
  `skills.config`. Project-scoped skill toggles refuse without a user-wide
  fallback. Matching path/name precedence is checked to avoid duplicate rules.
  Other rules and unknown settings stay.
- Claude standalone MCP: change only the selected project's
  `projects[<absolute-project>].disabledMcpServers` list in `<home>/.claude.json`.
  This is a per-project choice, even when `--scope user` selects a user-defined
  server. It is not a global disable and does not change approval lists.
- Codex standalone MCP: change `mcp_servers.<name>.enabled` in the exact
  definition's user or ancestor project TOML file.

A selector that also addresses another observed occurrence/scope refuses,
including same-agent aliases to one canonical skill. Plugin lifecycle/content,
unknown scopes, invalid/unsupported schemas, reserved Claude MCP names and
uncertain targets refuse unchanged. Safe shared-source skill toggles use native
config only; disabling by moving/editing that source is never attempted. Native
managed policy, trust, alternate setting sources and live session state remain
unresolved; the operation changes the stated local setting, not a running session.
Other settings may still restrict loading. Do not infer runtime visibility.

Config parent directories must already exist, be owned by the user/root and
not be group/other-writable. Create a missing agent config directory with its
native setup before retrying; Skillrail does not silently create authoritative
parent trees. Root-owned sticky temporary ancestors are accepted for fixtures.
Final configuration files must be user-owned, regular, single-link files without
write access for group/other. Symlinked configuration files/parents and extended
ACLs are refused. Intentional skill-source links remain read-only and intact.
TOML editing supports ordinary/inline tables and both rule array forms. Uniform
CRLF inputs retain CRLF; reparsing verifies that restoring line endings preserves
literal/multiline values. Mixed LF/CRLF edits refuse unchanged. JSON
patches replace only the selected value span or insert the missing object key.
All bytes outside that JSON span remain identical; whitespace inside the changed
value may normalize. Duplicate JSON keys refuse. Existing unknown keys,
credential values, owner/group and regular permission bits are preserved; new config files
use mode 0600. ACL-bearing paths and nonregular configs refuse. Other extended attributes and
file flags are not backed up/restored; this journal guarantees exact bytes and
ordinary ownership/permission bits, not every filesystem metadata extension.

TUI: `e` enable / `b` disable opens an occurrence picker showing agent, scope,
path and ID. Arrows select and Enter builds a preview. A second Enter applies;
Escape cancels. `h` opens history, arrows select an operation, `u` previews undo,
then Enter confirms or Escape cancels. PgUp/PgDn scroll modal views. Mutation
errors remain visible in the TUI; a successful apply refreshes inventory.
Ctrl-C quits and attempts restoration from every screen. Search text retains
the existing key behavior while search is being edited.

## Durable journal, history and recovery

`<state-dir>/journal` is durable, mode 0700. Its lock and JSON operation records
are user-owned mode 0600 regular files with exactly one link. New writes use
private schema version 2; version-1 records remain readable and are rewritten as
version 2 on recovery. The public schema/cache/MCP contracts remain version 1.
Version 2 stores each original/proposed byte sequence once; publication and undo
metadata refer to those bytes instead of duplicating them. Original
and proposed config bytes live only in these private records and private staging
locations; they are never indexed, served by MCP or printed by history/errors.
Do not copy journal files into a public directory. Keep the journal when deleting
`index.sqlite3`: rebuilding the cache does not recover a deleted journal.
Unsupported journal versions or corrupt records refuse safely; do not delete or
rebuild them as a cache. Record size remains capped at 4 MiB. Before admitting a
new operation, the exact serializer sizes a worst-case complete lifecycle with
all files published and all undo stages recorded, maximum-width future inode/owner
metadata, longest stage names and status. Oversized operations refuse before any
journal/config write. Repeated recovery reuses its existing plan and cannot grow
beyond that reservation. Aggregate history reads
at 64 MiB, with at most 10,000 operation IDs. Exact-ID undo does not enumerate the
whole history. Operations are returned deterministically by stable operation ID.

Apply checks all preconditions under the shared journal lock and durably records
intent, originals, postconditions and staging names before publishing any config.
Each config replacement is staged on its own filesystem inside a user-private
0700 sibling directory, preserving its original permission bits inside that
private directory. The stage is synced, its exact identity is recorded and synced,
then the target and stage are checked again before descriptor-relative rename.
Journal updates also use staged, synced replacements. Directory entries are synced.
Creating a missing target uses Linux `RENAME_NOREPLACE` / macOS `RENAME_EXCL` and
refuses unsupported filesystems; a late external creator cannot be overwritten.
Replacing an existing target publishes the complete new file by atomic rename.
Several files are **not** one filesystem transaction: each published replacement
can be observed before the next file changes.

`operations` exposes only IDs, states, safe target selectors, changed-file counts
and fixed recovery guidance. States are `prepared`, `applied`, `undoing`, `undone`.
If interrupted while prepared/undoing, new forward mutations refuse. Run:

```bash
skillrail --home /chosen/home --project /chosen/project --state-dir /chosen/state operations --json
skillrail --home /chosen/home --project /chosen/project --state-dir /chosen/state undo op-0123456789abcdef0123456789abcdef --json
skillrail --home /chosen/home --project /chosen/project --state-dir /chosen/state undo op-0123456789abcdef0123456789abcdef --apply --json
```

Use the original home/project context. Undo prechecks the complete set, restores
only exact matching published versions, leaves never-published originals alone,
removes files the operation created, and removes only matching stages. It retains
the original undo plan across repeated interruptions, reusing any exact matching
durable stage inode rather than deleting/recreating it. Recovery rebuilds an
unrecorded partial undo stage only when its bytes are a prefix of the
planned replacement; recorded stages still require exact bytes and identity.
Cleanup applies the same prefix check to unrecorded forward stages.
Repeated completed undo
returns unchanged. A conflict preserves intervening edits and leaves the durable
record for inspection; reconcile the conflicting file externally before retrying.
There is no force overwrite, auto recovery on scan, or automatic forward replay.
History is not a composable undo stack: if A disables, B enables, and undo B
restores A's bytes through a new inode, undo A still refuses its changed inode.
This conservative check preserves external-edit identity safety; byte equality
alone does not authorize an older undo. Preview a new desired toggle or reconcile
recovery explicitly instead of assuming cascade undo.

The lock coordinates Skillrail mutations, not external writers. Descriptor-relative
`openat(O_NOFOLLOW)`, owner/mode/ACL/link checks and repeated parent/identity/byte
checks detect observed substitution without following external symlink targets.
Existing-file replacement/unlink has no portable compare-and-swap tied to an
expected inode: an external same-user process can still edit/rename between the
last check and the syscall. Post-write checks report detected races; private
originals and planned postconditions remain recoverable. No guarantee is made
against hostile same-user/root writers, concurrent permission/ACL changes after
checks, network filesystem semantics, or storage that lies about fsync durability.
Do not concurrently edit mutation targets with native agents. macOS's descriptor
and exclusive-rename paths require actual CI/native validation; Linux results do
not certify them.

New errors: `mutation_refused` / `mutation_conflict` exit 4, `mutation_io` /
`journal_invalid` exit 1; malformed selectors/operation IDs exit 2; unmatched
item/occurrence or missing valid operation IDs exit 3. JSON errors retain the
existing stderr-only schema. Clap usage errors remain text. Interrupted records
can remain even when a failed apply published no target; inspect history and undo.


## Owned snapshot installation

`install` defaults to a preview. `--apply` explicitly publishes the previewed
snapshot and selected links. Repeat `--agent` to select both agents; each
selection applies to the single required `--scope user|project`. Claude links
use `.claude/skills/<frontmatter-name>`; Codex links use
`.agents/skills/<frontmatter-name>`, beneath the selected home or project.
No agent configuration is edited and no other agent/scope is implicitly selected.
Native trust/session visibility stays unknown.

```bash
skillrail install /absolute/local/skill --agent codex --scope user --json
skillrail install /absolute/local/skill --agent codex --scope user --expect '<preview-token>' --apply --json
skillrail install /absolute/local/repository --git --ref main --skill skills/review --agent claude --agent codex --scope project --apply --json
skillrail install https://github.com/example/skills.git --git --ref '<commit>' --skill skills/review --agent codex --scope user --json
```

Without `--git`, source is a local directory, including its working-tree bytes.
With `--git`, source is a local ordinary/bare repository or credential-free
HTTPS URL. Branch/tag/40-hex commit references use a conservative ASCII grammar;
blank means HEAD. Ref expressions, remote helpers, SSH/scp, HTTP, file URLs,
credentials/userinfo, ports, escapes, query strings and fragments refuse.
Use an absolute filesystem path for local Git. Git worktrees, shallow local
repositories, alternates, submodules and SHA-256 object repositories refuse.
There is no picker or prompt in the CLI. An omitted `--skill` is accepted only
when the source contains exactly one SKILL.md; otherwise provide the exact
relative directory, or `.` for the root. Selection does not install other skills.

New content lives exclusively under `<home>/.skillrail-sources/<owned-id>`;
`--state-dir` moves recovery/cache data but never the owned source store.
Links point to this snapshot, never the original source. Reinstalling an existing
snapshot or using any occupied destination refuses. Native name/invocation
collisions in the selected project context and case aliases also refuse.
External content keeps its owner and location. No force/overwrite or implicit
link addition to an earlier install is provided in M5.

The private snapshot `.skillrail/manifest.json` records schema 1, owned ID,
frontmatter name, exact snapshot/link/file paths and provenance. Git provenance
records the requested ref (default HEAD), resolved commit, selected subpath and sanitized repository;
local provenance records the absolute origin, subpath and content hash.
`skillrail-snapshot-v1` hashes the sorted JSON entry records (relative path,
bytes, executable boolean and directory boolean) using the application's
length-prefixed SHA-256 field encoding. Empty directories participate. `.git`
internals and added `.skillrail` bookkeeping do not participate. This digest is
separate from inventory's existing full-folder fingerprint algorithm.
Ordinary source permissions normalize to owner-only 0600/0700, retaining whether
a file is executable. Snapshot directories are 0700. Source scripts never run.
Later edits to the source do not change the installed snapshot.

The selected skill tree has at most 1000 entries, depth 32, 512 KiB aggregate file
bytes, 64 KiB SKILL.md, ASCII path segments up to 128 bytes, and a contained
subpath up to 1024 bytes. Both name and nonblank description are required. Every selected adapter is
validated before preview: Codex names have a 64-character maximum, including
dual-agent installs; its optional metadata must have the supported native shape.
The application retains its conservative ASCII name subset and 4096-byte
description budget. Claude alone reserves the case-insensitive `synced` folder
and exact `anthropic-skills` namespace; benign `anthropic-skills-helper` is allowed.
Codex-only installs may use these Claude-reserved names. Colon names remain
outside the application grammar for either adapter. Selection precedes content reads and validation. Explicit `--skill` resolves
only its exact directory/tree and guarded ancestors; unrelated large files,
Unicode paths, links and plugin layouts outside that subtree do not invalidate
it. Without selection, a separate bounded candidate walk/listing (10000 entries,
local depth 32, Git listing 1 MiB) finds SKILL.md paths without reading unrelated
file/blob bodies; ambiguity requires explicit selection. Git selected ancestors
must identify one exact tree, with no duplicate/case-aliased component or symlink.
All selected-tree symlinks (including internal links), hard-linked files, special
files, unsafe owners/permissions/ACLs, case collisions, Unicode paths, `.skillrail`,
plugin manifests and submodule metadata refuse. These conservative limits avoid unverified APFS normalization
and link semantics. `.gitattributes` is copied as inert data; filters are not run.

Anonymous HTTPS additionally requires that the OS account home has neither
`.netrc` nor `_netrc`: Git enables optional libcurl netrc lookup. The preflight
checks metadata only and refuses without reading values. `NETRC=/dev/null`
protects newer curl builds; the absence check protects older ones without
overriding HOME/CODEX_HOME. Empty-auth negotiation and credential delegation are
disabled. Concurrent creation of credential files after preflight is outside the
same-user race guarantee. See [Git HTTP source](https://github.com/git/git/blob/v2.56.0/http.c).

Git uses `/usr/bin/git` argument arrays with a cleared environment, no global or
system config, no templates/hooks, no credential helper or prompts, and only
HTTPS transport enabled. Local Git copies bounded objects/refs/HEAD/packed-refs
into a private bare repository and never parses the source config. The metadata
copy is limited to 64 MiB, 10000 cumulative entries across objects/refs recursion
and HEAD/packed-refs, and individual guarded files to 4 MiB. Entries are charged
before copying, including empty directories; HEAD/packed-refs bytes are charged too. HTTPS explicitly
fetches into the same isolated repository, with redirects/proxies disabled and
HTTP low-speed limits. No checkout, clean/smudge filter, submodule command,
source script, shell interpolation or inventory fetch occurs. Network/DNS/storage
costs of an explicit fetch are not a sandbox or fixed total transfer budget.
Operational Git errors discard raw stderr and return fixed safe text.
`tempfile` is now a runtime dependency to own and clean this private scratch.

Install plans bind the selected journal path as well as home/project and source.
Preview retains private content in memory and exposes only paths, manifest
facts, token and actions. JSON schema 1 adds nullable/omitted `install` to install
plans and `source.installed` to owned inventory sources. The latter contains
validated provenance, revision and content hash; content bodies never enter
inventory, SQLite or MCP. TUI `i` opens source/kind/ref/subpath/agents/scope fields.
Enter/Tab advances, Up goes back, Backspace edits, Escape cancels. Enter on scope
creates a retained preview; a separate Enter confirms apply. Source input is
hidden so rejected credential URLs cannot echo into the terminal. Existing
history/undo screens work with installs.

Install operations use private journal schema 3 under the existing lock; toggle
records keep schema 2 and schema-1/2 recovery remains readable. Schema 3 stores
source content once, with inode evidence separately. Admission serializes the
worst future metadata/parent chains against the same 4 MiB lifecycle budget.
Selected tree structure rejects dot/parent paths, duplicate entries, incompatible
mode/type pairs, file/directory prefixes and case aliases before preview. The
generated record is validated both at admission and before journal saves.
Intent is synced before parent/stage creation; a private stage tree is recorded
before writing files, complete file identities before exclusive tree publication,
and link identity before each exclusive link publication. The snapshot publishes
first, then links individually. Operations are recoverable, not atomic as a set.
An interrupted unrecorded file prefix or exact planned staged link can be cleaned;
unknown bytes/paths and changed recorded identities refuse. Undo first validates
the whole set, then removes matching links and content, retaining durable evidence
across interruptions and repeated undo. Newly created empty store/agent parents
are retained; undo removes only the owned tree and its stages/links.

Undo also refuses extra discovered consumers of the snapshot in the selected
home/project inventory. It requires complete, identity-bound scans of supported
Claude/Codex roots and relevant plugin/fallback sources; permission failures,
malformed roots, unknown candidates and enumeration/loading limits refuse.
Generic `coverage_partial` for known unsupported coverage is not itself a failure.
Install name checks require the same evidence for selected agents only.
Destination enumeration detects overflow explicitly and reserves capacity for its
stage/link. Case/native-name checks repeat immediately before publication. It cannot enumerate arbitrary filesystem references or
unrelated project contexts; do not create external consumers while an install
operation is being undone. The existing same-user/root, final-check race,
filesystem/exclusive-rename and fsync limitations apply to tree/link operations.
No macOS/native session or real HTTPS network validation is inferred from the
Linux local/local-Git fixtures. Those remain explicit integration prerequisites.

## Owned update and uninstall

`update <id>` and `uninstall <id>` default to preview. Add `--apply` to write;
`--expect <token>` refuses a changed preview. Select an exact owned ID (the
64-character `install.owned_id`), inventory item ID, or occurrence ID. A name,
unknown ID, ambiguous selection, plugin, external item, missing/edited manifest,
or ownership without matching durable installation evidence cannot authorize a
write. Use the original home/project and private journal context. A manifest
alone is insufficient to adopt an existing directory.

```bash
skillrail update '<owned-or-item-id>' --json
skillrail update '<owned-or-item-id>' --expect '<preview-token>' --apply --json
skillrail uninstall '<owned-or-item-id>' --json
skillrail uninstall '<owned-or-item-id>' --expect '<preview-token>' --apply --json
skillrail operations --json
skillrail undo 'op-<32-hex-digits>' --apply --json
```

Update reacquires the declared local/Git origin, requested reference and exact
previously selected subpath through the same bounded acquisition as install.
It validates metadata for the original selected agents. New source skills do not
change that selection. Agent locations, scope and skill name cannot widen or
change implicitly; a changed name refuses and requires explicit uninstall/install.
Git previews expose the resolved commit, including a provenance-only change when
a ref moves to a different commit with identical content. Updates never check for
upstream changes during inventory. An equal source and provenance is an
`unchanged` no-op with no new operation record. No-op apply still revalidates
ownership, tree, links, consumers and source. Local source directory identities
(including local Git roots), content and resolved provenance bind retained plans
and CLI expectation tokens. Reacquisition repeats under the lock and immediately
before replacement-snapshot publication.

Plan schema 1 adds `previous_install`, a validated original manifest, and
`content_diff`, a sorted list of every added/removed/changed full-folder entry.
Each diff entry contains `path`, `action` and nullable `before`/`after`. Each state
contains `kind` (`file`/`directory`), byte `size`, `sha256` (ordinary SHA-256 of file
bytes; null for directories) and normalized `mode` (JSON integers: 384 = 0600,
448 = 0700). Mode-only changes, binary files, scripts, references, added/removed
files and empty directories participate. Unchanged entries are omitted. For a
provenance-only update or no-op, the diff is empty/omitted. `install` is the new
manifest for update, the selected original for uninstall; `previous_install`
retains the original facts on undo too. File actions describe original retention,
replacement creation, link retarget/removal and undo restoration. No text hunks,
raw skill bodies, arbitrary config values, script contents or backup bytes enter
previews, history, inventory, SQLite or MCP. Paths, sizes and hashes intentionally
reveal structural/content equality facts; this is a payload-free content diff,
not a facility for viewing source text.

Both operations require exact owned tree/file/directory identities and content,
manifest bytes, and every original selected link. They refuse unexplained edits,
missing links and unowned consumers. Relevant supported Claude/Codex roots and
plugin/fallback scans must be complete, including occurrence fingerprints.
Unknown extra files, incomplete scans, aliases and ownership conflicts preserve
existing content. Publication repeats no-follow identity, collision, consumer and
stage checks at the actual rename boundary. No agent configuration is rewritten.
A current project's arbitrary external references and unrelated projects cannot
be exhaustively enumerated; the selected supported inventory context bounds these
checks. Concurrent external writers, final-check/syscall races and filesystem
and fsync constraints remain those described above.

Private journal schema 4 extends the same lock and guarded filesystem operations;
schema 1/2 toggles and schema 3 installs remain recoverable. Original links move
by exclusive rename into private `.skillrail-<operation-id>-original` directories
inside their agent roots, with the backup-directory identity saved before each
move. The original tree moves to the same private basename in the source store.
These retained originals keep their exact inode, content, manifest, modes and
provenance. Update then uses the existing install publisher to stage a new
snapshot at its provenance-derived owned ID and publish the original selected
links to it. Uninstall stops after retaining the original content/links: the
public snapshot/link paths disappear while private recovery content stays. Paths
can be temporarily absent between individual renames; this is not atomic across
files, links or agents.

Complete original and proposed bytes are stored once per lifecycle record with
separate identity evidence. Admission reserves worst-case future tree/file/link
metadata and backup-directory chains within the existing 4 MiB private record
limit, plus directory enumeration capacity for backup/stage/publication. A large
install may fit its original install record but refuse an update requiring both
versions. Such a refusal occurs before original retention/publication; unchanged
source no-ops do not need a new lifecycle reservation. Existing 64 MiB/10000-record
history limits remain. There is no automatic backup/history garbage collection:
keep the journal and retained original trees/links together. Deleting private
backups can strand undo even when the journal still exists. Retention uses disk
space until recovery restores the original; applied history is deliberately not
pruned behind the user's back.

Undo first validates the whole set. A completed update requires its complete
replacement snapshot and exact published links; completed uninstall requires the
originals still retained and public paths absent. Intervening edits, replacement
identities, extra backup paths or unknown consumers of original/replacement/staged
content refuse unchanged. Interrupted prepared/undoing records permit only
matching partial publication/cleanup states. Recovery removes verified replacement
content and restores original tree and link inodes by exclusive rename, preserving
provenance precisely. Repeated interruptions retain progress, including death
before stage identity or halfway through a new file write. Completed undo is a
no-op on repetition. No force overwrite, filesystem-wide atomicity, historical
cascade or automatic forward replay is provided. Undoing a later lifecycle can
restore the exact earlier snapshot/link identities, allowing earlier undo only
when its original postconditions still match; this does not relax config-toggle
inode checks.

TUI: select an owned item with arrows/search/filters, `u` previews update and `D`
previews uninstall. The same full diff and manifest facts appear in the retained
preview; PgUp/PgDn scroll, Enter confirms, Escape cancels. `h` lists operation
actions/states; select an operation and press `u` to preview undo, then Enter to
confirm or Escape to cancel. MCP remains the same five read-only tools.
