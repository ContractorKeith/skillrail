# Skillrail — grill scratch notes

## Confirmed before round 1
- Name: skillrail.
- Rust, Ratatui, SQLite; existing files are authoritative and the index is rebuildable.
- V1: Claude Code and Codex; global and project discovery; provenance, symlink tracking, drift reporting; TUI, JSON CLI, read-only MCP queries; deliberate install/enable/disable operations.
- Later: other agent adapters, filesystem watching, online discovery.
- Worktree and agent-session launcher is a separate project.
- Follow the Basecamp idea-to-milestones process (document 10464058936).
- Interview only; no implementation or scaffold yet.

## Round 1 — confirmed
- Prioritize a dependable daily Claude Code/Codex tool plus milestone/review workflow practice.
- Release gates cover Linux and macOS; Windows is outside v1.
- Preserve existing skill locations, dotfiles ownership, and symlinks; no mandatory central store or migration.
- Inspect plugin-provided skills and MCP entries for origin and visibility; native tools own plugin installation, updates, and toggles.

## Round 2 — confirmed
- Separate source identities; group copies or symlinks only with provenance evidence. Matching names alone flag possible collisions.
- Visibility means configured discoverability for a project, not proof of loading or use by a live session. Represent unknown explicitly.
- Inspect global locations plus one selected project, defaulting to the current directory.
- Drift compares full skill-folder content across related local copies, including scripts and references. No upstream network checks; Git revisions are provenance.

## Round 3 — confirmed
- Existing dotfiles, npx skills, and plugin owners retain content-update ownership; Skillrail may change supported per-agent visibility.
- Skillrail-owned installs use one user-level source directory with links into selected global/project agent locations. Existing installations remain in place.
- Refuse unsupported per-agent toggles when implementing them would affect other consumers.
- Preview mutations and persist undo records; undo refuses to overwrite intervening edits.

## Round 4 — confirmed
- MCP status describes configuration only: present, enabled/disabled where supported, invalid/unknown. No live connection checks.
- Toggle existing standalone MCP entries with preview and undo; adding/copying entries and plugin management stay with native tools in v1.
- Exclude credential values from the index, TUI, JSON, and MCP responses; preserve them during edits. Recovery bytes requiring credentials remain in private local files.
- Skillrail's read-only MCP surface exposes inventory, search, provenance, visibility explanations, drift, and paths. Serving skill bodies and bundled files is deferred.

## Round 5 — confirmed
- Repository/local-path installs copy the selected skill into a Skillrail-owned snapshot with source and content hash, then link to selected agents. Source edits never silently update the snapshot.
- Explicit update and uninstall for Skillrail-owned installs, with preview and undo. Updates show content changes; uninstall removes only owned content and links.
- TUI and CLI offer the same operations. TUI confirms a preview; CLI previews by default and requires --apply to mutate.
- Scan on TUI startup and manual refresh; scan freshly for each CLI/MCP inventory request. Show the TUI's last scan time.

## Round 6 — confirmed
- Release requires automated Linux/macOS tests with isolated agent homes, TUI interaction tests, and real Claude Code/Codex smoke checks with disposable configurations.
- Gates cover interrupted writes, concurrent edits, broken symlinks, credential leakage, and undo conflicts, plus independent PR reviews and the release skeptic from the Basecamp process.
- Distribute Linux/macOS release binaries, a Homebrew formula in ContractorKeith's tap, and source installation through cargo. No self-updater.
- Milestone implementer: Codex gpt-6.1-sol, medium effort, subject to availability. Astra owns orchestration and integration. Do not substitute unavailable models without Keith's decision.
- ALL milestones run sequentially, including after M0. This overrides the suggested parallel cadence.

## Handoff
- Keith confirmed the shared understanding and authorized /spec, /tickets, and the sequential build on 2026-11-02, using new T3 threads and Sol 6.1 workers.
- Canonical specification: docs/PLAN.md. Implementation starts at M0 after ticket creation.
- Preserve the Basecamp process: one milestone issue, branch, draft PR, and isolated worker per milestone; full-diff review, independent reviewer, adversarial pass, and release skeptic at the documented gates.
- Resolve adapter mechanics against current official Claude Code/Codex documentation during specification; unsupported visibility controls must remain explicit, never simulated by changing shared sources.
- No ADR created: the current decisions are recorded here; none has yet been established to meet all three ADR criteria.
