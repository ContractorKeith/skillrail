# Standalone skill compatibility (M1)

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
excluded. All plugin and MCP discovery remains M2. No legacy path is guessed.

## Identity, ownership and credential boundaries

Canonical folder targets prove symlink aliases. Separate canonical folders
are related only when local `.git/config` supplies one unambiguous `origin`
HTTPS URL and the same repository-relative skill subpath. This is declared
local Git evidence, **not authenticated upstream identity**. Remote config is
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
