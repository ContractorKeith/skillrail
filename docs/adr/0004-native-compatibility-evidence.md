# 0004: Native agent compatibility rules and evidence semantics

Status: accepted (recorded from code comments, 2026-11-27)

## Context
Skillrail explains what Claude Code and Codex are configured to see, but it
can't observe a live session. Every rule below either mirrors pinned native
behavior or refuses to guess. docs/compatibility.md has the user-facing
limits.

## Decisions
### Absence is not evidence
- A dangling link, an unreadable ancestor, or a missing child under a dangling
  link is a failed root, not an empty one (`discovery::confirmed_absent`,
  `mcp_inventory`). A broken or unsafe `.claude` parent doesn't prove skills
  are absent.
- Unsupported runtime restrictions stay as explicit uncertainty. Settings are
  local observations, never proof of effective session visibility. Installs
  are tracked separately from enablement and runtime visibility.
- `list` and `scan` rebuild from current files and never return stale rows.
  The TUI only filters the last explicit scan and never discovers sources.

### Discovery
- When home equals project, the same location isn't a second installation.
- A directory without `SKILL.md` isn't a skill. Broken links stay as rows.
- Without a repository boundary, only the explicitly selected project is
  supported.
- A full-folder fingerprint includes execute bits. Timestamps, UID and write
  bits don't define content. Internal links are recorded, never followed.
- Repository identity requires both the origin and the same relative subpath.
  Linked worktree gitfiles and symlinked Git metadata need separate evidence.

### Claude
- Main-checkout fallback (`src/worktree.rs`): any `.claude/skills` directory in
  the root suppresses fallback, even an empty one. `None` means fallback
  doesn't apply. `Err` means applicability is unknown. Only the standard
  linked-worktree layout is supported, and both pointers must agree.
- Claude reserves `synced` (case-insensitively), `anthropic-skills` and the
  exact synced namespace. A link's folder name must equal the frontmatter
  name.
- Claude also documents plugins installed under its skills directory. Those
  roots are inspected only for plugin candidates, so ordinary skill aliases
  don't produce plugin-link diagnostics.
- Claude adds declared plugin skill roots to the default root. Codex's
  compatibility manifest selects the declared root.
- In a project file, absence means inheritance, not an explicit "on" rule.
  A project observation isn't automatically a trusted override.

### Codex
- The native skill-rule resolver reads only the user layer and session flags.
  Project and system `skills.config` layers are ignored, so toggles write
  `~/.codex/config.toml`.
- Skill names follow pinned Codex `6ff1e1ca` `skills/src/parser.rs`: at most 64
  characters, a non-blank sanitized description, typed optional metadata.

### Plugins and MCP
- Plugin MCP server names are namespaced. A toggle never disables an
  unrelated definition with the same name, and a native selector that matches
  several physical occurrences is refused rather than widened.
- A manifestless Claude plugin gets its identity only from validated
  installation evidence. Portable components stay canonical. An OpenAI
  overlay isn't an extra MCP source, and an unsupported overlay is diagnosed
  without emitting its raw values. Plugin lifecycle stays native.

### Query
- A query returns indices into one snapshot, and matching occurrences
  constrain each other. All occurrences of a matching item are kept, so
  explanations stay complete.
- The page revision hashes sanitized facts and deliberately leaves out the
  timestamp, so any change invalidates the pages.
