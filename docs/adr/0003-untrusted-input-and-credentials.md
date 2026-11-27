# 0003: Untrusted input, credentials and Git isolation

Status: accepted (recorded from code comments, 2026-11-27)

## Context
Agent configs can contain credentials, and skill sources can be hostile. No
credential value may reach output, SQLite, errors or previews (AGENTS.md).
Nothing may run hooks, filters or helpers.

## Decisions
- **Allowlisted facts only.** MCP configs are deserialized into a transient
  tree, and only allowlisted facts cross the module boundary
  (`src/mcp_inventory.rs`). A raw config is never serialized. URL checks
  validate structure without keeping any URL fragment.
- **No echo of parser context.** Malformed input can contain a credential
  inside the parser's error context. The MCP server installs no tracing
  subscriber for that reason. Evidence scripts withhold native stderr
  entirely.
- **Duplicate JSON keys are ambiguous**, not last-value-wins
  (`src/json_config.rs`).
- **Reject, don't redact.** Credential-bearing or ambiguous remote URLs are
  rejected, and no arbitrary URL field escapes (`provenance::safe_remote`).
- **Narrow Git config grammar** (`src/provenance.rs`). Skillrail reads local
  Git evidence only: no Git commands, includes, hooks or remote requests.
  Section and key names are case-insensitive, but quoted subsections aren't.
  Deprecated dotted remote syntax is rejected, so another origin can't be
  overlooked. Even ignored keys are validated, because malformed config isn't
  evidence. Unknown syntax is never normalized.
- **Isolated Git acquisition** (`src/install_source/git.rs`). Objects are read
  in an isolated bare repository with no checkout. Git never parses a local
  source repository's config, includes, helpers, hooks or alternates. The tree
  is resolved one component at a time, never following a Git symlink and never
  silently picking one of several duplicate or case-aliased ancestors. Shape
  validation happens before any blob read or journal admission.
- **Anonymous HTTPS.** Git enables `CURL_NETRC_OPTIONAL`, and older curl
  ignores `NETRC`. So the OS account's implicit credential files must be
  absent before Skillrail fetches. Their values are never read.
- **Bounded, non-executing reads** (`src/files.rs`). Reads use `O_NOFOLLOW |
  O_NONBLOCK`, so a substituted FIFO can't block and a substituted final
  symlink isn't followed. SQLite opens its cache with no-follow as well.
- **Ownership is offline.** Manifest ownership is read only from the fixed
  user store `~/.skillrail-sources`.
- **Terminal output** escapes every untrusted value (`src/presentation.rs`).
- **Worktree layout paths** are rejected if symlinked, before any pointer file
  is opened. `..` is resolved only after validation, because symlinks change
  what it means.
- **Evidence scripts** pass through only `HOME`, `CODEX_HOME` and `TMPDIR`. No
  provider keys, routing, injected settings, Git wrappers, telemetry
  endpoints or token-bearing variables.
