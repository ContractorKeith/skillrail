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

Codex path selectors currently match the original or canonical SKILL.md path
lexically. Alternate symlink spellings and paths containing `..` are not
normalized; their native-agent semantics have not been verified. A missing
disable observation is not evidence of enablement; visibility remains unknown.

M1 reports configured visibility as `unknown` with concrete observations and
reasons. It does not claim that any session loaded a skill. User-level Codex
TOML is bounded and parsed to explain matching enable/disable rules. A matching
user disable is an observation, not proof that no higher-priority setting can
override it. Project Codex config makes rule interpretation uncertain because
trust/config layering is unresolved. Unsupported or malformed selectors never
become a positive visibility claim. Configuration values and parser failures
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

[Claude manifest reference](https://code.claude.com/docs/en/plugins-reference)
establishes default skills plus declared roots, and default `.mcp.json` then
ordered declarations with whole-entry replacement inside that plugin. Root
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
