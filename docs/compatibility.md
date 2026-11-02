# Agent inventory compatibility (M1–M2)

Evidence checked 2026-11-02 on Linux x86_64, Rust 1.98.1:
`claude --version` → `2.1.309 (Claude Code)`;
`codex --version` → `codex-cli 0.166.0`.
Only version commands were run. No real agent configuration, authentication,
credential stores or skills were inspected. These are documentation-backed
adapters with executable synthetic fixtures, **not real-agent load tests**.
macOS and disposable real-agent verification remain pending.

## Official sources and supported locations

- [Claude skills](https://code.claude.com/docs/en/skills): personal
  `<home>/.claude/skills/<folder>/SKILL.md`, plus `.claude/skills` at the
  selected directory and its ancestors through the nearest repository boundary.
  Personal command precedence is tracked per invocation (frontmatter and folder
  names), so a conflicting folder alias does not imply the whole skill is hidden;
  nested project skills can coexist. Symlinked standalone folders are supported.
- [Codex skills](https://developers.openai.com/codex/skills/) redirected on the
  check date to [Build skills](https://learn.chatgpt.com/docs/build-skills).
  The adapter uses `<home>/.agents/skills` and ancestor `.agents/skills` through
  the repository root. Names do not select a single winning source. Symlinked
  skill folders are supported. User `skills.config` rules select an absolute
  SKILL.md path or a name, with the last match winning.
- [Claude settings](https://code.claude.com/docs/en/settings) and
  [Codex configuration](https://developers.openai.com/codex/config-basic/)
  explain additional policy/configuration layers. Skillrail does not resolve
  their complete runtime combination; it must not infer visibility from file
  presence alone.

A `.git` file or directory marks an ancestor boundary without executing Git.
Outside a repository only the selected directory is scanned. Descendants and
unrelated projects are not traversed. Each supported root enumerates immediate
skill folders, not arbitrary recursive skill trees. Original occurrence paths
and canonical source paths are both retained. Symlinked root/folder targets
are intentional read roots; links *inside* a skill obey the fingerprint boundary.

### Linked worktrees and invocation names

The [monorepo/worktree discovery rule](https://code.claude.com/docs/en/skills#load-skills-in-monorepos-and-subdirectories)
adds Claude's main-checkout `.claude/skills` when a linked worktree lacks a root
`.claude/skills` directory (documented for v2.1.277+). A present directory, even
empty, suppresses fallback. Selected nested project roots still apply. Codex
gets no main-checkout fallback. Occurrence paths name the actual main-checkout
entry and canonical source, with a fallback observation in visibility reasons.

Resolution reads at most 8 KiB each from the worktree `.git` file and its Git
administrative `commondir` and `gitdir` pointers. The normal
`<main>/.git/worktrees/<id>` layout must agree in both directions. Absolute and
relative pointers are supported; each path has at most 256 components. Symlinked
metadata paths, malformed/missing/unreadable/over-budget pointers, ambiguous
skills-root absence and separate/bare Git-directory layouts yield
`worktree_fallback_unknown`. Raw pointer contents never reach output/cache. No
Git process runs during inventory. Tests use actual disposable orphan worktrees,
with hooks and system/global Git configuration disabled during fixture creation.
The fixture gate requires Git with `worktree add --orphan` support.

[Claude command naming](https://code.claude.com/docs/en/skills#how-a-skill-gets-its-command-name)
allows a standalone folder name to invoke a skill in addition to its frontmatter
name. Each occurrence now includes sorted, deduplicated `invocations` with
`name` and `shadowed_in_default_layout`. This is a personal/project precedence
observation for that invocation only. It never establishes source identity or
proves runtime visibility. Different frontmatter names in two `deploy` folders
therefore retain distinct identities while explaining personal precedence for
`/deploy`. Codex retains its metadata name without a Claude folder alias.

## Visibility and unsupported combinations

The M1 visibility-reason matcher compares Codex path selectors to the original
or canonical SKILL.md path lexically. Alternate spellings are not normalized in
that older explanatory path. M4 observations and mutation selectors canonicalize
paths using the pinned native source below. A missing
disable observation is not evidence of enablement; visibility remains unknown.

M1 reports configured visibility as `unknown` with concrete observations and
reasons. It does not claim that any session loaded a skill. User-level Codex
TOML is bounded and parsed to explain matching enable/disable rules. A matching
user disable is an observation, not proof that no higher-priority setting can
override it. Pinned native source now establishes that project `skills.config`
is ignored; only user/session layers supply these rules. Unsupported or malformed
selectors never become a positive visibility claim. Configuration values and parser failures
are never emitted or cached.

Claude personal/project shadowing, invocation frontmatter, and presence of
settings are explained. Claude permission rules, skillOverrides, managed
policies, trusted project config, setting-source selection, safe/bare modes,
CLI overrides and session state are not evaluated. Neither adapter resolves
custom agent homes/environment variables, extra directories, enterprise/admin
locations, bundled skills, profile selection, or remote/account sessions.
Unsupported linked-worktree metadata layouts remain explicit uncertainty.
Coverage is explicitly partial even when empty.
No `/etc` agent roots are read, preserving explicit fixture isolation.

The current Claude documentation confirms old `.claude/commands` files, but
M1 inventories folder skills only; command files are explicitly unsupported.
Codex `.codex/skills` legacy paths are not inventoried: the current official
skills page does not establish their compatibility semantics. Claude `synced`
is excluded as account content; skills-directory plugins are diagnosed and
excluded from the standalone adapter. M2 adds the plugin/MCP subset described below. No legacy path is guessed.

## Identity, ownership and credential boundaries

Canonical folder targets prove symlink aliases. Separate canonical folders
are related only when local `.git/config` supplies one unambiguous `origin`
HTTPS URL and the same repository-relative skill subpath. This is declared
local Git evidence, **not authenticated upstream identity**. Remote config is
accepted only in the supported ASCII grammar; non-ASCII configuration and
unsupported whitespace conservatively fall back to canonical identity.
Remote config is
never executed; includes, SSH/SCP/file remotes, Git config quoting/expansion,
linked-worktree gitfiles (for source identity), symlinked Git metadata and
complex config formats are unsupported. Discovery fallback uses only the bounded
linked-worktree relationship described above; it does not expand provenance
claims. Unsupported provenance falls back to canonical identity with an explicit
unsupported/unavailable/unsafe evidence reason. Name/hash equality never joins
sources. Revisions and upstream availability are not inferred.

Only a narrow HTTPS host/path grammar is accepted. Userinfo, query, fragment,
port and ambiguous escaping reject the whole remote, rather than printing a
partially redacted URL. No raw config, command arguments or environment values
enter inventory or SQLite. Source identity is a length-delimited SHA-256 digest
of the evidence. Occurrence identity includes agent, scope and original path.
External repository/unknown ownership never transfers to Skillrail.

## Metadata and fingerprints

`SKILL.md` must be a regular, UTF-8 file of at most 64 KiB, with YAML frontmatter.
Claude can fall back to the folder name; Codex requires name and description.
Names use the conservative ASCII alphanumeric/underscore/hyphen subset (128
bytes); descriptions have a 4096-byte limit. Unsupported metadata stays a row
with diagnostics. Unknown YAML keys are ignored, not executed. Skill bodies,
hooks, dynamic commands, scripts and optional runtime metadata are never run.
M1 does not expose skill bodies. Raw parser errors are discarded.

`skillrail-folder-v1-sha256` hashes a sorted, length-delimited representation of
relative UTF-8 paths, file/directory/link type, execution bits, regular file
bytes and literal link targets. All scripts, references, assets and hidden
content are included except entries named `.git` and `.skillrail` (source-control
internals and reserved Skillrail bookkeeping). Empty directories count.
Timestamps, UID/GID, inode numbers, non-executable permission bits, extended
attributes and ACLs are excluded so copies can compare reproducibly.

Internal symlinks are recorded but never traversed: their target must resolve
inside the canonical skill root; its content is covered at its ordinary tree
location. Broken, looping, root/escaping links, special files, unreadable paths,
non-UTF-8 paths or exceeded budgets invalidate the entire digest. Limits: depth
64, 10000 entries per skill/root, 16 MiB per file, 64 MiB total folder bytes;
config limits are 64 KiB Git and 128 KiB Codex. Incomplete fingerprints are null,
never evidence of equality. A single physical source is `single_source`, related
complete copies are `equal` or `drifted`; any incomplete copy makes drift `unknown`.

Scans are bounded observations, not atomic filesystem snapshots. File changes
observed during reads invalidate them; concurrent edits can still race directory
walks. Do not concurrently rewrite source trees while comparing copies. No
upstream calls or source writes occur. The cache directory must be trusted
against planted hard links, as documented in the CLI contract.

## Dependencies

- [serde-saphyr 2.0](https://docs.rs/serde-saphyr/2.0.0/serde_saphyr/): maintained
  typed YAML parsing with built-in resource budgets; no tag-driven execution or
  include resolver is enabled. Handles frontmatter without a hand-written YAML parser.
- [sha2 0.11](https://docs.rs/sha2/0.11.1/sha2/): pure Rust SHA-256 for stable
  identities and full-folder fingerprints without shell interpolation.
- [toml 1.1](https://docs.rs/toml/1.1.8/toml/): parses actual Codex user rule
  syntax, including arrays of tables, without editing configuration.

`Cargo.lock` pins integrations. Tests observe executable scans and TUI buffers,
not the existence of this document. `scripts/accept.sh` runs the full fixture
gate; release, real-agent and later milestone gates are not implied by M1.

## M2 evidence and supported installation selection (2026-11-02)

This Linux x86_64 worker used Claude Code **2.1.309**, Codex CLI **0.166.0**,
Rust **1.98.1**, and Git **2.56.0**. Official documentation is living evidence,
not a versioned schema guarantee. No real user configuration, credentials or
skills were inspected. Native Claude evidence below uses a generated local
marketplace and disposable home/project/state; no model session or MCP server
is started. macOS and broader native loading verification remain pending.

### Claude installation evidence

[Official plugin loading](https://code.claude.com/docs/en/plugins/loading)
documents install-record scope/path/version, cache versions and old-version
retention. `python3 scripts/claude-plugin-evidence.py` reproduces the native
schema probe (requires exactly Claude 2.1.309 and a built Skillrail binary).
It sets only Claude's documented `CLAUDE_CONFIG_DIR` isolation override;
HOME and CODEX_HOME are never reassigned. All native writes are under its
small disposable fixture. It creates its own Git boundary, disables Git global/
system configuration, and requires exactly four replay occurrences so ancestor
content cannot enter the result. Commands run there:

```
claude plugin marketplace add <fixture-local-marketplace> --json
claude plugin install personal@fixture-market --scope user --json
claude plugin install shared@fixture-market --scope project --json
claude plugin install private@fixture-market --scope local --json
claude plugin install personal@fixture-market --scope project --json
claude plugin update personal@fixture-market --scope project --json
```

Native output established `.claude/plugins/installed_plugins.json` with envelope
`{"version":2,"plugins":{"name@marketplace":[<records>]}}`. Each record carries
`scope`, absolute `installPath`, string `version`, `installedAt`, `lastUpdated`,
and `projectPath` for project/local scopes. Updating the fixture left personal
user scope at 1.2.3 and project scope at 2.0.0; both cache directories remained.
The executable probe prints the normalized generated registry and replays it
through Skillrail with explicit `--home`, `--project`, `--state-dir`.

The adapter accepts that versioned envelope, validates all required types,
correlates install paths against the ID/version cache layout and retains
user/project/local scope for the selected project. Repeated scoped records
produce separate occurrences. Other-project records do not acquire selected
scope or leak their project paths. Unselected versions remain `residual` cache
observations. Missing, duplicate, malformed, over-budget or unknown-version
registries produce explicit uncertainty instead of guessed installation facts.
Optional `gitCommitSha` is accepted only as a string and discarded, like dates.
Unsupported registry fields/layouts are diagnosed. Registry-validated cache-root
links may resolve an installed source; arbitrary cache links are refused.
Non-cache/in-place records whose layout is not established remain unknown.

### Codex versioned source evidence

The official `openai/codex` tag **rust-v0.166.0**, resolved commit
**6ff1e1ca3b30fc8b914720681fcf5fe62e95f856**, provides exact evidence:

- [installed.rs](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/core-plugin-common/src/installed.rs)
  defines selection: valid version directories, `local` preferred, otherwise
  maximum version using SemVer comparison when both parse and lexical comparison
  otherwise. Skillrail uses the same `semver` crate to preserve prerelease and
  multi-digit ordering rather than inventing a comparator.
- [store.rs](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/core-plugins/src/store.rs)
  places `.codex-remote-plugin-install.json` at the plugin base, not a version
  directory. Its schema is `schema_version:1` and nonempty `remote_plugin_id`.
  Skillrail validates it, emits no remote ID, and diagnoses unknown metadata.
- [mcp_types.rs](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/config/src/mcp_types.rs)
  pins environment-selector forms, approval enums and scopes. The
  [official configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
  corroborates `env_vars` strings or `{name, source?}` with local/remote source,
  string-array `scopes`, and approval modes auto/prompt/writes/approve.

The supported Codex store exposes version selection and remote identity
metadata, not Claude's scoped central install registry. Codex installation
scope therefore stays unknown; configuration-layer scope is separately known.
A selected store version does not prove activation or live-session loading.
Remote/account identity reconciliation and custom homes are not inferred.

### Configuration observations and components

[Claude MCP](https://code.claude.com/docs/en/mcp) establishes standalone user and
selected-project local entries in `.claude.json`, project `.mcp.json`, and
local > project > user > plugin definition precedence. The selected project's
persisted `disabledMcpServers` list is retained, including namespaced
`plugin:<manifest-name>:<server>` matching. User/project/local settings'
`disabledMcpjsonServers` and `enabledMcpjsonServers` are separately recorded with
source/scope; a known rejection changes the configured status to disabled.
Approvals are not assumed trusted. Managed policy, setting-source selection,
tracked-file approval eligibility and session state remain unresolved.

Codex user and ancestor project TOML layers retain their own plugin activation
and per-server enablement observations. Unrelated project model settings do not
erase a user disable. Conflicting known observations summarize as unknown while
both facts remain visible. Malformed containers, per-server tables, booleans,
arrays and supported approval enums produce sanitized diagnostics/invalid
status. Nested or unknown policy remains explicitly unknown. A matching rule
never makes an invalid or unsupported server definition valid.
Standalone Codex definitions reject transport-incompatible fields (such as
HTTP headers on a command, or arguments on a URL) and require unsigned integer
`startup_timeout_ms`, matching the pinned native parser.

[Claude manifest reference](https://code.claude.com/docs/en/plugins-reference)
establishes default skills plus declared roots, and default `.mcp.json` then
ordered declarations with whole-entry replacement inside that plugin. Root
components are also discovered without the optional Claude manifest when a
validated v2 installation record establishes plugin identity. Missing manifests
without that identity remain unknown; malformed manifests are not treated as
absent. The independent Claude 2.1.309 fixture installed a manifestless plugin
with `skills/example/SKILL.md` and `.mcp.json`; replay covers both occurrences.
Root
`SKILL.md` is one plugin skill only when there is no `skills/` and no `skills`
key. This fallback inventories only the root skill and does not enumerate or
fingerprint unrelated plugin content; its full-folder digest is unknown.
Skills-directory plugins use user or selected-project scope, not ancestor
plugin discovery. Managed trust and desktop main-checkout plugin behavior remain
unresolved. M1 standalone skill worktree fallback is unchanged.

[OpenAI packaging](https://developers.openai.com/plugins/build/plugins) establishes
Codex's marketplace/plugin/version cache, portable `plugin.json`, `skills/`,
`mcp.json`, and compatibility `.codex-plugin/plugin.json`. Portable components
remain canonical; OpenAI presentation/hooks/app overlays are not inventoried.
Compatibility skill roots and local/inline MCP components are supported.
Marketplace component overrides, bundles, registered app mappings and arbitrary
external plugin directories remain explicitly unsupported.

### Resource and credential boundaries

Plugin loading has scan-wide limits: 128 inspected candidates, 256 declaration
nodes (including nested arrays/repeated paths), 1000 entry/tree operations,
256 file reads, and 1 MiB of content reads. Registry and install-metadata reads,
manifest/components, skill metadata and fingerprints consume the shared budget.
Every entry enumerated under declared skill roots consumes that same entry
budget, including ordinary files that do not become skill candidates.
A JSON file additionally has a 128 KiB ceiling. Existing bounded cache-root
enumeration remains 1000 entries per agent cache. One `plugin_budget_exceeded`
diagnostic marks truncation; final plugin MCP status becomes unknown, with no
per-entry budget-error amplification. Item/occurrence aggregation is grouped
rather than repeatedly scanning every occurrence per item.

All JSON configuration rejects duplicate keys conservatively, even in nested
objects. Native duplicate-key behavior is unverified: rejection is an explicit
unknown, not a claim that the native agent rejects it. Parse context and values
never reach output. Unsupported fields/transports and nested auth/tool schemas
also stay unknown. URL checks establish only a structural minimum, not native
URL acceptance; no endpoint part is retained.

Component paths must be contained plugin-relative paths with no parent
traversal, variables or backslashes. No scan executes commands/helpers, expands
values, extracts bundles or contacts a network. Raw configuration trees remain
transient. Typed sanitized data alone feeds CLI/TUI/SQLite. New layer/registry
canaries, cumulative-budget fixtures and native evidence complement the M1
provenance, terminal escaping, cache and worktree regressions.

## M3 read-only MCP protocol evidence

The server uses the official [Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
crate **rmcp 3.8.0** (Cargo.lock pinned), with server/async-stdio features;
client/child-process support is test-only. No HTTP/auth transport is enabled.
Tokio supplies asynchronous stdio and a limited blocking scan worker. These
additions avoid maintaining custom negotiation/protocol models and allow an
actual official SDK client to verify the executable.

The implemented session lifecycle follows the official
[lifecycle specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle),
[stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
and [tools contract](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).
The server explicitly limits negotiated versions to **2024-11-05, 2025-03-26,
2025-06-18 and 2025-11-25**, rather than advertising every newer SDK version.
All five tools use one text block containing the version-1 query JSON; no
resources, prompts, writes, subscription or live MCP health capabilities are
advertised. Tools retain configured visibility uncertainty from M1/M2.

`tests/mcp_protocol.rs` uses rmcp's client/child-process transport against the
binary for initialize/list/every read tool, safe errors, fresh scans, CLI parity,
source-byte/credential canaries, and no configured command/network execution.
`scripts/mcp-smoke.py` probes all four negotiated versions plus unsupported
version fallback, malformed syntax, structurally invalid JSON-RPC, 64-KiB input
limits, unknown tools/resources, argument errors, responsive protocol/busy errors during
a real locked-cache scan, and EOF both idle and with a scan in flight. The SDK intentionally
ignores syntax-invalid/oversized lines and returns safe Invalid Request for
structurally invalid messages; this behavior is tested and documented, not
claimed to be a custom parser. `scripts/accept.sh` runs both these checks and
real PTY search/detail/refresh/navigation/resize/terminal-restoration checks.

Local M3 evidence is Linux x86_64 with Rust 1.98.1. No macOS or native agent
session execution is claimed by this implementation run. The same acceptance
script runs in Linux/macOS CI; exact CI outcomes and independent/adversarial
reviews remain integration checks. Existing native adapter evidence above is
unchanged and does not establish MCP session loading.

### M3 transport cancellation regression

rmcp 3.8.0's `AsyncRwTransport::receive` clears a parsed malformed line before
awaiting the writer lock and error-frame flush. Its service loop selects that
receive future alongside outgoing completion events. Cancelling receive while
it waits can therefore discard the only error response for an already-consumed
line. A controlled one-byte async writer regression reproduced the loss on Linux
without Python, subprocess timing or longer timeouts. It failed with
`cancelled receive lost the malformed-request response` before the adapter.

Skillrail retains the pinned official parser/framing behind a persistent reader
task and capacity-one request channel. Outgoing SDK framing and parser errors
share a writer lock for each complete frame. The reader is aborted on drop and
aborted/awaited on close; I/O failures release frame locks. Five transport tests
cover cancellation while waiting for output, partial error flush, partial input,
close/drop under backpressure, and write failure cleanup. No dependency, registry
source or protocol-parser changes are needed. Raw stdio smokes also pipeline
malformed requests with normal replies; `--repeat 1..20` bounds repetitions.
macOS acceptance on the corrected commit remains required CI evidence, not a
local Linux claim.

A separate supplied macOS run stopped at the PTY exit wait rather than MCP.
A controlled burst of 100 alternating page redraws reproduced that timeout on
Linux: waiting without draining output filled the PTY and prevented terminal
restoration/exit. The smoke now drains while waiting under the original ten-second
deadline, and a bounded backpressured-quit scenario verifies attributes and the
alternate screen are restored. This changes the test driver, not TUI behavior.

## M4 native toggle evidence and support

M4 evidence checked 2026-11-02 on Linux x86_64, Rust 1.98.1 / Git 2.56.0.
The installed Claude executable now reports **2.1.316 (Claude Code)**; the
older M2 **2.1.309** inventory evidence above remains historical evidence.
M4 pins the inspected 2.1.316 native Linux executable SHA-256:
`fd13c70000fec3f2116dfbcbb6121210b4a29328bf0e46210a59dd594a67d824`.
No model session, real config, authentication or credential store was read.
Only `--version` and executable-code inspection were used for this evidence.

`python3 scripts/toggle-evidence.py --claude-binary /absolute/native/claude`
checks that exact version/hash and native embedded setting definitions and
toggle logic, then fetches two exact Codex source files at the pinned commit.
It requires that specific Linux binary, rustc and network access to raw.githubusercontent.com;
an unavailable/different prerequisite fails explicitly. It is an evidence probe,
not part of offline acceptance and not a native session-loading test. The layer
probe compiles the exact pinned `skill_config_rules_from_stack` function with
small typed fixture collaborators: user/session rules participate, project/system
rules do not, and session overrides user. Removing the source's filter is a
negative control that must fail. This exercises the filter rather than merely
looking for selector strings.
The probe passed locally with:

| Source | SHA-256 |
|---|---|
| Codex `config/src/skills_config.rs` | `df7e8bcd53b9aef6309a930a25203bfb052345b71091b4e2000e200dedeb2d6b` |
| Codex `config/src/mcp_types.rs` | `af5fb824c092cd74e3a42297979a4d250c332008f5409e14890253728747e71a` |

The [official Claude skill visibility settings](https://code.claude.com/docs/en/skills#override-skill-visibility-from-settings)
document `skillOverrides` values `on`, `name-only`, `user-invocable-only`, `off`,
with absent = on, and explicitly exclude plugin skills. The pinned binary's
embedded settings schema declares the same four states. Its toggle code writes
`disabledMcpServers` with membership `!enabled`, corroborating the
[official per-project MCP toggle](https://code.claude.com/docs/en/mcp#disable-a-server-without-removing-it)
in `.claude.json`. This list is distinct from project-MCP approval/rejection lists.
These mechanisms change native configuration without changing externally owned
skill contents or MCP definitions.

Codex evidence remains tag **rust-v0.166.0**, commit
**6ff1e1ca3b30fc8b914720681fcf5fe62e95f856**:

- [skills_config.rs](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/config/src/skills_config.rs)
  defines path/name plus boolean `enabled`, canonicalizes path selectors, consumes
  only User and SessionFlags layers (explicit filter in the rule resolver),
  deduplicates identical selectors and applies later matching rules last.
  Project rules are ignored even when trusted. Skillrail writes canonical
  absolute document paths only for supported user-scoped toggles and leaves
  broad name rules untouched. Project-scoped toggles refuse unchanged.
- [mcp_types.rs](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/config/src/mcp_types.rs)
  defines `McpServerConfig.enabled:bool`, default true. Skillrail patches this
  field in the selected standalone definition, never a guessed `disabled` field.
- The [official config reference](https://learn.chatgpt.com/docs/config-file/config-reference)
  corroborates path-based `skills.config` enablement and per-server enablement.

| Target | Native mechanism / write location | Supported limit |
|---|---|---|
| Claude standalone user skill | name → on/off in `<home>/.claude/settings.json` | Refuse another Claude occurrence with the same native name |
| Claude standalone project skill | name → on/off in selected `.claude/settings.json` | Selected-project override; ancestor source content remains untouched |
| Codex standalone user skill | canonical SKILL.md path + enabled in user `skills.config` | Refuse another Codex occurrence addressing that same canonical source |
| Codex standalone project skill | none | Native resolver ignores project skill rules; refuse without user-wide fallback |
| Claude standalone user/project/local MCP definition | selected project's `.claude.json` `disabledMcpServers` | Per-project effect even for user definitions; no same-name competing occurrence or reserved name |
| Codex standalone user/project MCP definition | exact definition's TOML `mcp_servers.<name>.enabled` | Refuse competing same-name occurrence/layer or uncertain definition schema |
| Plugin skills/MCP or unknown scope | none | Refuse unchanged; use native lifecycle |

Cross-agent skill aliases can be toggled safely with these native mechanisms:
only the selected agent's supported config changes. Claude project enable
accounts for known inherited user/ancestor off/name-only/user-invocable-only
states by writing explicit on; known restrictive local overrides refuse. Same-agent
aliases/collisions that a native selector would affect together refuse; Skillrail does not move a shared
source or silently widen the requested scope. Native profile, CLI, managed-policy,
trust and session precedence remain explicit uncertainty. Known local skill
settings now appear as `skill_enabled` observations with their exact source
path/scope; ignored Codex project skill rules do not become enablement
observations. Visibility itself remains unknown. The earlier lexical-only M1
explanation remains conservative, while M4 observations and writes use the pinned
canonical selector evidence.

JSON editing preserves untouched spans; TOML uses locked **toml_edit 0.25.17**
for comment/layout-preserving edits, including arrays of tables and inline rule
arrays. Existing keys and credentials stay in their files. `serde_json`'s borrowed
`RawValue` feature locates JSON value spans after the existing duplicate-key
check. **libc 0.2.191** supplies descriptor-relative guarded operations and native
exclusive-rename flags. These additions avoid guessed config serialization and
check-then-follow filesystem writes. Extended ACL-bearing mutation paths refuse
rather than dropping ACLs. Darwin ACL declarations follow Apple's
[sys/acl.h](https://github.com/apple-oss-distributions/Libc/blob/main/include/sys/acl.h)
and [entry implementation](https://github.com/apple-oss-distributions/Libc/blob/main/posix1e/acl_entry.c);
Linux checks POSIX access/default ACL xattrs through the open descriptor.

### Journal isolation and bounded guarantees

Issue #5's cache hard-link carry-forward is implemented separately: the durable
journal does **not** inherit the cache's trusted-directory-only assumption.
Every opened recovery file is checked for owner, mode 0600, regular type and link
count one. The recovery directory and config staging directories require 0700;
all parents are opened component by component without following links, and
owner/mode/ACL and descriptor identities are checked. Existing config files may
retain read permissions but cannot be group/other-writable or hard-linked.
Original config bytes are only in private recovery records/staging; history and
MCP never expose them. Cache hard-link limitations documented earlier still
apply to SQLite; deleting that cache leaves operations/undo intact.

The journal records intent and original bytes before staging/publishing and saves
the stage identity before every publish. Both file and containing directories are
synced. Private stage directories are siblings of the destination, so renames stay
on its filesystem. Multi-file work is recoverable, not atomically visible as a set.
History reports prepared/applied/undoing/undone with an explicit undo recovery
path. Private schema 2 stores originals/proposals once and references them from
stage/undo metadata; schema-1 recovery records remain readable. Admission sizes
the exact worst-case complete lifecycle against the unchanged 4 MiB limit,
including all files, future maximum-width metadata and complete undo progress.
It refuses before journal/config mutation when that reservation does not fit.
Undo retains its plan and reuses exact durable stages across repeated crashes;
conflicting user edits refuse unchanged. Unknown record versions fail closed.
Strict inode preconditions intentionally prevent composing historical undos:
A(disable), B(enable), undo B succeeds, but undo A refuses the replacement inode
even when its bytes equal A's postcondition. No identity handoff/force cascade
is introduced. Uniform CRLF TOML edits retain line endings with value equivalence
checked after conversion; mixed styles refuse without touching config bytes.

Linux uses `renameat2(RENAME_NOREPLACE)`, macOS uses
`renameatx_np(RENAME_EXCL)` when the destination was absent; unsupported filesystem
support refuses rather than reverting to an overwriting rename. Existing-file
replacement has no portable expected-inode compare-and-swap. An external process
can still race after the final check; descriptor anchoring prevents following its
symlink target, postchecks expose observed changes, and recovery content remains.
The same-user/root process model, concurrent ACL/permission changes, filesystem
and storage durability limits are stated in the [CLI contract](CLI.md#durable-journal-history-and-recovery).
No protection against hostile same-UID/root access is claimed.

### M4 verification and pending prerequisites

The shared acceptance gate retains all 70 M0–M3 tests and the SDK cancellation
regressions/bounded adapter. M4 adds public CLI/application tests for exact previews,
wrong targets, safe shared-source toggles and plugin/collision refusals, byte and
credential preservation, history and conflict undo, repeated operations, no-op
revalidation, cross-process locks, deterministic inherited-lock reproduction,
symlink/hard-link/parent/stage substitutions, ACL/private modes, exclusive create,
interrupted multi-file apply and repeated interrupted undo, cache rebuild and a
real SDK client against the journal-bearing state. Linux ACL fixtures require
POSIX ACL xattrs. Fault injection is a typed application callback, never an unsafe
production environment flag. PTY checks drain output while waiting and cover
occurrence choice, preview, cancel, confirm, history, undo preview/cancel/apply,
credential canaries and terminal restoration.

Local evidence is Linux only. macOS compilation/execution, Darwin ACL behavior,
exclusive-rename/filesystem support and disposable native session-loading checks
remain explicit integration/release prerequisites; no native loading claim is
made from source inspection or fixtures. No prerequisites are silently skipped
by `scripts/accept.sh`; the optional pinned-source probe is reported separately.
Independent full-diff and adversarial review remain the parent's gate before ready
or merge. Install/update/uninstall remain M5/M6, with no MCP writes added here.

The first M4 macOS CI compile at b2f61e9 found E0617: Darwin's 16-bit `mode_t`
cannot be passed directly as a C variadic argument to `openat`. The follow-up
uses the promoted `c_uint` argument on both platforms. Linux acceptance was
rerun after this fix; macOS rerun results remain an integration check.

### M4 round-2 corrections and local evidence

The parent-reviewed `3802a76` PTY isolation fix is retained. Linux acceptance after
round 2 passes **110 tests** (the original 70 plus 40 M4), strict Clippy/fmt,
CLI/cache, SDK/MCP cancellation/limits and real PTY smokes. macOS acceptance
for this new head remains CI evidence, distinct from the parent's green
Linux/macOS baseline at `3802a76`. The strengthened native-source probe passed
with its removed-layer-filter negative control. No native session loading was
inferred.

Failing-first regressions reproduced the unsupported Codex project write,
source-retarget token reuse, inherited Claude off/no-op, accepted 200 KB settings
that exceeded the old undo budget, recorded-stage inode replacement during real
process interruption, and CRLF normalization. Tests now cover native user positive
controls, project no-write refusal, stable/source-bound tokens, inherited
on/off/name-only/user-invocable-only and local override refusal, single/batch
capacity boundaries (500 KB/250 KB per-file positive controls and larger early
refusals), schema-1 recovery, and interrupted/repeated undo. Linux's process test
uses a fixture-scoped fsync interposer and real exit 77/78 interruptions; its fresh
stage control proves the interposer fires, while recorded stages survive retries.
The Linux test requires the existing C compiler prerequisite; portable stage
recovery tests remain active on macOS. Sequential historical undo remains
conservative, with explicit byte-preservation/conflict coverage.


## M5 owned install evidence and limits

Linux x86_64 fixtures exercise local directories and local Git object repositories,
exact commit/subpath manifests, immutable copies, selected Claude/Codex user/project
links, explicit selection, offline inventory provenance, credential-URL refusals,
collisions/aliases, executable/binary/empty-directory preservation and guarded
recovery. A Linux `write` interposer terminates an actual process halfway through
snapshot staging; durable undo removes only its matching prefix. Typed checkpoints
cover tree/link publication, boundary substitutions and interrupted/repeated undo.
The real PTY smoke covers source and explicit selection, form/preview cancellation,
apply, history, undo cancellation/application and terminal restoration.

The [CLI install contract](CLI.md#owned-snapshot-installation) lists exact syntax,
source budgets, ASCII-only names, all-source-symlink refusal, local ordinary/bare
Git support and credential-free HTTPS-only remote support. Git plumbing runs in
private bare metadata; source configuration, hooks, filters and scripts are not
executed. Inventory remains offline with five unchanged read-only MCP tools.
`source.installed` exposes validated snapshot provenance; it does not serve bodies
or claim native session loading. Existing owners are never adopted by name/hash.

The user source store is `<home>/.skillrail-sources` on both platforms. Recovery
uses the existing private journal/lock with additive schema 3; schema 1/2 remain
readable. Empty created parent directories remain after undo. Unknown discovered
consumers in the selected context prevent undo; references outside that inventory
are not discoverable guarantees. Native agent location evidence remains the M1/M4
pinned evidence above, without new claims about live-session loading.

Local execution is Linux only. macOS build/acceptance, APFS case/normalization,
Darwin link-stat/ACL/exclusive-rename behavior, real credential-free HTTPS fetches,
and disposable native agent sessions remain unverified integration/release checks.
No tests contact real network repositories. HTTPS needs `/usr/bin/git`, its HTTPS
transport/TLS trust and network access, plus absence of implicit netrc files in
the OS account home. Netrc metadata is checked without reading credential values;
HOME/CODEX_HOME are never overridden. No authentication helper or redirect
fallback is provided. Independent Standards/Spec review and adversarial QA belong
to the parent before ready/merge; this implementation run does not certify them.


### Simultaneous resize/input readiness correction

At M5 head `2243862`, one macOS CI job timed out in the first PTY smoke's quit
wait while its sibling run passed. The log did not capture the event ordering.
Linux reproduced the exact timeout with a syscall scheduling probe that coalesced
real SIGWINCH/input readiness and returned the resize notification first. In
Crossterm 0.29's default Mio reader, `try_read` returns a Resize event from inside
the readiness loop, discarding the remaining terminal readiness notification.
With edge-triggered polling the unread quit byte can then remain stranded.
Output draining does not fix a lost input notification.

Skillrail enables Crossterm's supported `use-dev-tty` feature, selecting its
level-triggered Unix poll reader. Unread descriptors remain ready on the next
poll. This uses already-locked `filedescriptor 0.8.3`; it adds no package/version
and avoids maintaining a custom input parser or dependency-source patch.
The standard PTY driver additionally sends resize immediately before quit on
both platforms. `scripts/pty-readiness-smoke.py` uses the existing C compiler
prerequisite on Linux to force simultaneous real readiness through the same
application/PTY path. It verifies the control actually fired and terminal
attributes/alternate screen restored after a single quit. Re-enabling the default
reader is a negative control reproducing the original ten-second timeout; the
level-triggered reader passes. Deadlines are unchanged.

This establishes a real event-reader defect consistent with the CI symptom,
not proof of the failed macOS job's exact event order. macOS execution of this
follow-up remains CI evidence; the Linux syscall interposer is explicitly
platform-specific and the ordinary resize/input smoke runs on both platforms.


The M5 follow-up also caps local Git metadata cumulatively at 10000 entries
across objects/refs recursion and HEAD/packed-refs, charging directories/files
before copying and including HEAD/packed-refs bytes in the 64 MiB budget. The
parent's failing-first fixture used 10302 empty directories to prove that
per-directory limits alone did not bound work. Combined Linux acceptance passed 135 Rust tests, strict fmt/Clippy, existing
smokes and the additional portable/deterministic resize-input PTY regressions.
