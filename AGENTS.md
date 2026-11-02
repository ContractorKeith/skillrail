# Skillrail

Local-first skill and MCP registry for Claude Code and Codex: Rust, Ratatui,
SQLite, JSON CLI and read-only MCP. Files are authoritative; explain provenance,
configured visibility and local drift. V1 is done when docs/PLAN.md's acceptance
checklist passes on Linux and macOS.

## Status
M0 and M1 merged in PRs #9/#13 — 2026-11-02. M1 standalone inventory passed
independent reviews/attack and Linux/macOS CI. Next: M2 MCP/plugin inventory.
See docs/compatibility.md for coverage and pending real-agent release checks.

## Commands
`bash scripts/accept.sh` is the shared worker/reviewer/CI gate (Rust + Python 3).
`cargo run --locked -- --help`, `cargo test --locked`, `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`.

## Pointers
- docs/CLI.md: implemented commands, JSON, paths and cache/error contracts.
- docs/PLAN.md: canonical spec, contracts, milestones and acceptance.
- CONTEXT.md: glossary; docs/grill.md: confirmed interview decisions.
- Session state: ../worklog/projects/skillrail.md. No repo-local worklog.

## Conventions & Gotchas
- One application core for TUI/CLI/MCP; one guarded mutation layer.
- Never modify real user agent configurations or skills during tests. Use
  explicit --home/--project/--state-dir fixtures; do not overwrite HOME/CODEX_HOME.
- Credential values never enter public output, SQLite, errors or previews.
- Existing owners retain content; unsupported per-agent toggles refuse unchanged.
- Docs and compatibility evidence must match implementation. Never mark a gate
  passed when its prerequisite was unavailable.
- Commits: `<type>: <description>`, ContractorKeith only, no AI attribution.

## Tracking
One issue/branch/draft PR/new T3 worktree thread per milestone, ALL sequential.
Workers use Codex gpt-6.1-sol medium; no nested delegation or worklog writes.
Astra owns integration, full-diff review and acceptance. Independent reviewer
and adversarial pass before ready/merge; release skeptic before tag.

## Out of Scope
Other agents, watchers, online discovery, plugin lifecycle, live MCP health,
external content updates, skill-body serving and worktree/session launching.
