---
name: skeptic-env
description: Isolate Skillrail release verification from real agent configuration and skills.
---

# Skillrail skeptic environment

Read AGENTS.md, docs/PLAN.md and docs/compatibility.md when present. Run
`bash scripts/accept.sh` first. Use temporary directories and explicit
`--home`, `--project`, `--state-dir` for every Skillrail command. Never change
the shell HOME/CODEX_HOME or write into real Claude/Codex configuration.

Probe same-name unrelated skills, broken and escaping symlinks, folder drift,
plugin provenance, unknown visibility, malformed configuration and canary
credentials. Verify preview/apply/undo, interruption and concurrent external
edits preserve unrelated bytes. Test install/update/uninstall with local Git
fixtures and no hooks or skill execution. Exercise TUI events/PTY and stdio MCP.

On macOS include case/Unicode aliases and canonical temporary paths; on Linux
include permission failures without relying on root permission behavior.
Real-agent checks use disposable configuration and record versions. Missing
auth/tooling is an unmet gate, never a pass; do not copy credentials.

Write the report to the orchestrator's requested worklog/reports path. Do not
publish a release, merge, log a session, or start other agents.
