# Skillrail — product specification and build plan

Status: approved scope, implementation pending (2026-11-02). The six-round
interview is recorded in [grill.md](grill.md); terminology is in
[CONTEXT.md](../CONTEXT.md). This is the canonical spec, not a second copy of a
GitHub issue. Keith authorized specification, tickets, and the sequential build.

## Problem statement

People using Claude Code and Codex cannot easily tell which skills and MCP
configurations apply to a project, which source owns them, or whether related
copies have diverged. Equal names do not prove equal sources; a symlink is not
a second installation. Existing tools already manage multiple agents, so merely
putting toggles in a table is insufficient. Skillrail must explain its results
and make deliberate changes recoverable while respecting existing ownership.

## Solution

A local-first Rust binary with a Ratatui interface, JSON CLI, SQLite index, and
read-only stdio MCP server. Inspect global locations plus one selected project.
Show source identity, occurrences, content owner, configured visibility with
reasons, name collisions, drift, and scan time. Files remain authoritative; the
index can be discarded and rebuilt. Unknown facts stay unknown.

V1 serves Claude Code and Codex on Linux and macOS. It is both a daily tool and
a practice ground for verified milestone delivery. Existing locations are not
migrated. New Skillrail-owned installs are snapshots in a user-level source
directory, linked into selected agent locations. Preview every mutation and
provide persistent conflict-aware undo.

## User stories

1. List skills for the current or explicitly selected project, distinguish
   global/project/plugin origins, and explain why each agent can discover one.
2. See distinct same-named skills as collisions, not falsely merged copies;
   see symlink aliases as occurrences of the same physical source.
3. Compare related local copies across the entire skill folder, including
   scripts/references, without contacting upstream repositories.
4. See malformed metadata, missing targets, unreadable files, unsupported
   configuration and uncertain visibility as actionable diagnostics, rather
   than silently missing rows or false claims that a session loaded a skill.
5. Inspect standalone and plugin-supplied MCP entries without starting a
   command, making network requests, or revealing embedded credentials.
6. Search, filter, and inspect the same inventory in a terminal, scripts, and
   an agent's MCP queries. Agents get paths and explanations, not skill bodies.
7. Toggle a supported per-agent skill or standalone MCP entry without changing
   another agent's access; an unsupported operation explains its limitation.
8. Preview and apply a change, then undo it. If another program edits a target
   after preview or application, refuse conflicting writes without losing it.
9. Install a selected skill from an explicit repository or local path as an
   owned snapshot, choosing agent and scope; never overwrite an existing item.
10. Explicitly update an owned snapshot with a content diff or uninstall its
    owned content and links; leave external installs and unexpected edits alone.
11. Recover from an interrupted multi-file change with a durable record of
    what happened and a safe next action, not an undocumented partial state.
12. Install a tested release on Linux/macOS through binaries or Homebrew, or
    build from source; understand compatibility and known limitations.

## Implementation decisions

### Architecture and test seam

Use a small Rust application core shared by CLI, TUI, and MCP. Agent adapters
own discovery, precedence, capability explanations, and configuration patches.
The index owns only queryable, credential-safe facts. A single mutation layer
owns previews, locks, preconditions, durable recovery records, writes and undo.
Interfaces never bypass that layer. Start with one library and binary, not a
workspace of speculative crates. The highest common seam is the application
API, proven externally with subprocess CLI tests; TUI events and MCP protocol
tests call through the same behavior.

SQLite is a rebuildable cache, never the only copy of install ownership or undo
records. Store durable installation manifests and operation records separately
from the cache. Scans must not install, execute skill content, resolve secrets,
or contact networks. TUI scans at startup and on refresh; CLI/MCP reads scan
freshly per request. Index replacement must not publish half a scan. Failed
partial scans preserve explicit diagnostics and do not imply absent items.

### Paths, identities, and visibility

Resolve paths through one explicit application configuration. Support
`--home`, `--project`, and `--state-dir` overrides so every test can isolate
agent locations, cache, sources, and recovery records without changing the
process's real HOME or toolchain settings. Defaults use the user's home,
current directory, and appropriate XDG/platform data/cache locations. Explain
which paths were used. Document environment/config precedence if added; no
hidden cwd-dependent writes to a different project.

Adapters must document a tested compatibility matrix, official source links,
agent versions, discovery locations, precedence, and unsupported cases. Follow
the installed agent's supported schema rather than guessing from another
tool's behavior. Include relevant ancestor/project scope and trust restrictions
in visibility explanations; if trust/session information is unavailable,
visibility is unknown. Do not scan arbitrary unrelated projects or launch
agent commands during inventory. Plugin caches are observed, never owned.

Identity uses verified provenance: canonical source target, repository plus
skill subpath when established, and owned installation manifests. A matching
name or content hash alone never proves common provenance. Occurrence IDs
include agent, scope, and location; stable item IDs must not expose credentials.
Unknown origins stay separate. Canonicalization must retain the original path
for explanations and diagnose broken links without traversal loops.

Full-folder hashes use a deterministic sorted representation of relative paths,
file bytes, and meaningful file/link metadata. Exclude source-control internals
and Skillrail bookkeeping explicitly; do not silently exclude scripts/assets.
Define symlink traversal boundaries and report inaccessible or unsafe content
as unknown drift rather than identical. Related copies with unequal content
are drifted; revisions are provenance, never proof of current upstream state.

### MCP entries and credentials

Inventory standalone entries and entries supplied by installed plugins.
Configuration status is present/enabled/disabled/invalid/unknown, not health.
An entry belongs to its agent, scope and origin; equal server names do not
prove equivalent endpoints. Only supported standalone entries may be toggled.
No add/copy server management or plugin toggles in v1.

Construct credential-safe output by allowlisting fields. Never index raw
configuration blobs, environment values, headers, credential-bearing URLs,
command arguments, raw parse failures, or credential values in previews,
logs/errors, JSON, TUI, or MCP responses. Preserve original config bytes/unknown
settings when editing. Recovery files that must contain original bytes are
private user-only files in a private directory; they are never query results.
Do not claim arbitrary strings are safe solely because their key lacks 'token'.

### Mutation protocol

Preview is non-mutating with respect to managed files/config/links. Apply
revalidates exact preconditions under an application lock, rejects changed
targets, stages same-filesystem atomic replacements, and writes a durable
operation record before publishing changes. External agents do not share our
lock: guard against races and revalidate at the actual write boundary; explain
remaining filesystem constraints. Multi-file operations are journaled,
recoverable operations, not falsely advertised as filesystem transactions.
Never follow an unexpected symlink while writing config or recovery files.

Undo reverses only an operation whose postconditions still match; conflicting
edits are preserved and reported. Repeating apply/undo must not duplicate
content or silently reset another change. Unsupported toggles fail unchanged.
Externally owned skills may have visibility changed only through a verified
per-agent mechanism; shared source files are never moved or edited to fake it.
Plugin management and external content updates remain in their native tools.

Install copies a selected skill into the owned source store and links to chosen
global/project agent locations. For repositories, resolve and record an exact
commit and subpath; for local directories, record origin and content hash.
Explicit install/update can fetch the requested repository; inventory cannot.
Use system Git via argument arrays, disable hooks/unsafe protocols, never run
repository or skill scripts, and reject traversal/escaping links and collisions.
No search/discovery marketplace or arbitrary shell command interpolation.
Updates operate only on manifests proving ownership, preview content changes,
and refuse unexplained local edits. Uninstall verifies ownership and retains
enough private content to support undo. Do not erase unknown files or consumers.

### Interfaces and contracts

The planned command surface (refine syntax without changing behavior):

```
skillrail [tui]                         # interactive inventory
skillrail version
skillrail list [--kind skill|mcp] [--agent claude|codex] [--json]
skillrail search <query> [--json]
skillrail show <id> [--json]
skillrail drift [--json]
skillrail scan [--json]                 # fresh scan / rebuildable index
skillrail enable <id> --agent <agent> [--apply] [--json]
skillrail disable <id> --agent <agent> [--apply] [--json]
skillrail install <source> --agent <agent> --scope user|project [--apply] [--json]
skillrail update <id> [--apply] [--json]
skillrail uninstall <id> [--apply] [--json]
skillrail undo <operation-id> [--apply] [--json]
skillrail operations [--json]
skillrail mcp                          # stdio, read-only
```

Global paths are usable with every command. Scope/subpath/ref selection must
be explicit when ambiguous; a noninteractive command never blocks on a picker.
Every read supports JSON with a documented schema version, deterministic
ordering, item IDs, scan time and diagnostics. Separate diagnostics from
protocol stdout. Every CLI mutation previews unless `--apply`; TUI requires
preview confirmation. No mutation tools are exposed via MCP.

Exit codes: 0 success (including preview), 1 operational failure, 2 invalid
arguments, 3 item not found, 4 conflict/refused/unsupported operation. Unknown
inventory state is represented in output, not a fabricated success status.

TUI includes searchable skill/MCP views, agent/scope filters, details with
origin/visibility reasons/drift, refresh time, mutation preview, operation
history and undo. Normal navigation and resize must work in small terminals;
restore terminal state on error/exit and sanitize terminal control characters.
Install/update/uninstall are available through the TUI, not just shell hints.

Read-only MCP tools: list inventory, search inventory, get item/details,
explain visibility, list drift. Results share the CLI's safe schema. Validate
inputs, negotiate supported MCP versions, handle pagination/limits, keep
stdout strictly protocol, and terminate cleanly on disconnect. Use the official
Rust SDK where suitable; test with a real client, not only matching custom code.

### Dependencies and why

- Ratatui and Crossterm: terminal rendering/events without building a UI toolkit.
- SQLite through rusqlite (bundled if needed): portable queryable local cache.
- Clap: consistent argument parsing and machine-readable CLI behavior.
- Serde/JSON and a TOML-preserving editor: typed output and safe agent config edits.
- A maintained YAML parser: skill frontmatter validation without executing tags.
- SHA-256 library: stable content fingerprints without shelling out.
- Official MCP Rust SDK: protocol negotiation and transport correctness.
- tempfile for isolated tests. Add other dependencies only with a concrete reason.

## Feature matrix

| Feature | Decision | Milestone |
|---|---|---|
| Isolated paths, schema, empty inventory, CI/gate | v1 | M0 |
| Skill discovery, provenance, visibility, collisions, drift | v1 | M1 |
| Standalone/plugin MCP + plugin skills, credential-safe status | v1 | M2 |
| Consistent search/details/filters and read-only MCP surface | v1 | M3 |
| Preview/apply/undo and supported skill/MCP toggles | v1 | M4 |
| Local/repository snapshot installation and agent links | v1 | M5 |
| Owned update/uninstall, content diffs, recovery | v1 | M6 |
| Linux/macOS release, Homebrew, real-agent smokes, skeptic | v1 | M7 |
| Other agent adapters, filesystem watcher, online discovery | Later | — |
| Upstream update checking, serving skill bodies over MCP | Later | — |
| MCP entry add/copy and plugin lifecycle management | Later | — |
| Worktree/session launcher | Separate project | — |
| Self-updater, executing installed skills, live MCP health checks | Out of v1 | — |

## Data model

Item: stable ID, kind, name, safe description, source identity/evidence, owner.
Occurrence: item, agent, scope, original path, canonical target, configured
visibility and reasons, capabilities, plugin origin if applicable.
Fingerprint: source/item, deterministic folder digest, completeness/diagnostics.
Scan: project context, timestamp, adapter versions, items/occurrences/diagnostics.
Install manifest (durable): owned ID, resolved source/ref/subpath, fingerprint,
owned snapshot and links, revision history sufficient for undo.
Operation (durable/private): ID, intent, preconditions, postconditions, staged
changes, backups, progress/status and recovery action. SQL is not its sole home.

## Testing decisions

`bash scripts/accept.sh` is the common local/worker/reviewer/CI gate. It runs
format checking, strict Clippy, tests, build and executable smokes. Automated
fixtures isolate all paths through explicit overrides; never repurpose HOME
or CODEX_HOME in the shell. Fixtures cover both agents and Unix platforms.
No real credentials, user skills or configuration may be modified by tests.

Prove observable outcomes at application/CLI boundaries: two same-name sources
stay distinct; symlink aliases resolve; unknown states remain explicit; deleting
the cache and rescanning reproduces inventory; a changed script causes drift;
malformed config produces safe diagnostics; planted secrets never reach output
or SQLite; refused operations leave all files byte-identical; previews write
no managed state; apply/undo preserves unrelated config and survives conflict,
interruption, concurrent process attempts, and symlink substitution.

Use Ratatui buffer/event tests plus a PTY smoke for navigation, resizing,
preview/cancel/apply/undo, and terminal restoration. Use a real MCP client for
initialize/list/call/error/EOF lifecycle. Seed fixtures, not tests of strings
in documentation or private helper implementation. Record failing-first proof
for bug fixes and behavior tests where appropriate.

Release acceptance also requires disposable real Claude Code/Codex
configuration checks on Linux/macOS, recording exact versions and commands.
Do not read credential stores or copy real authentication into fixtures.
Unavailable prerequisites are reported as pending gates, never simulated pass.
Independent PR review and adversarial pass precede each merge; a fresh release
skeptic runs before the version tag. Confirm packaged binaries and formula
against release checksums and smoke them after extraction/installation.

## Milestones and dependency map

Every milestone is one issue, branch, draft PR and new T3 worktree thread.
ALL milestones run sequentially. Blockers below reflect reused behavior, not
permission to launch parallel work after a blocker clears.

| Milestone | Outcome | Depends on | Observable proof |
|---|---|---|---|
| M0 | Harness/schema/seams/CI | None | Empty isolated inventory through CLI and TUI; gate on Linux/macOS |
| M1 | Explain skill inventory | M0 | Fixture inventory, collision/alias/drift and diagnostics in CLI/TUI |
| M2 | Explain MCP/plugin inventory | M1 | Plugin origin and safe MCP states in CLI/TUI; secret canaries absent |
| M3 | Query inventory from agents | M2 | Real MCP client reads same facts as CLI/TUI; no write tools |
| M4 | Reversible visibility changes | M3 | Toggle preview/apply/undo through CLI/TUI; conflicts preserve bytes |
| M5 | Install owned snapshots | M4 | Local and Git fixtures install into store and selected agents; undo |
| M6 | Update/uninstall owned skills | M5 | Diff/update/uninstall/undo including ownership and recovery failures |
| M7 | Release v0.1.0 | M6 | Full gates, real-agent evidence, skeptic, four release targets and tap |

## Out of scope

No migration of existing content, external-owner content updates, unknown
configuration rewrites, shared-source moves to fake toggles, or implicit
enabling of every agent. No claim about what a running session loaded. No
plugin lifecycle management, MCP server execution/health checking, cloud
registry, skill-body serving, watcher, or launcher in v1.

## Further notes

- Scope reference: [skillz](https://github.com/olekmorawski/skillz), MIT/Rust,
  already multi-agent; adopt visibility explanations, not wholesale sync.
- [skills-tui](https://github.com/ChindanaiNaKub/skills-tui), TypeScript/Ink,
  demonstrates restore UX; no declared license found, so study behavior only.
- Process reference: sibling mdkb and [Basecamp build process](https://app.basecamp.com/6307587/buckets/49448999/documents/10464058936).
- Adapter starting sources: [Codex skills](https://developers.openai.com/codex/skills/),
  [Codex MCP](https://developers.openai.com/codex/mcp/),
  [Claude skills](https://code.claude.com/docs/en/skills),
  [Claude MCP](https://code.claude.com/docs/en/mcp). Revalidate concrete fields
  against the pinned compatibility versions when implementing adapters.
- Workers: Codex gpt-6.1-sol, medium. Astra orchestrates and integrates.
  No model substitutions without Keith's decision. No nested delegation.
- The orchestrator reads the whole diff, reruns acceptance, validates independent
  reviewer and attacker findings, makes PRs ready, and merges by merge commit.
  GitHub issues are the task ledger; one Basecamp card mirrors milestones.

## Acceptance checklist

- [ ] M0–M6 observable proofs pass on Linux and macOS.
- [ ] Every v1 feature works through its specified interfaces.
- [ ] Adapter compatibility and unsupported cases are documented with evidence.
- [ ] No secret canary appears in outputs, logs, index, or nonprivate recovery.
- [ ] Preview, conflict, interruption and undo tests preserve unrelated data.
- [ ] Disposable real-agent checks and independent release skeptic are complete.
- [ ] Packaged binaries and Homebrew pass smoke checks; docs match release.

There are no unresolved product decisions. Adapter compatibility mechanics are
implementation evidence to obtain, not reasons to invent unsupported behavior.
