# 0001: No code comments; decisions live in ADRs

Status: accepted, 2026-11-27

## Context
An audit of all 328 comments found that most were decision records squeezed
into one-line asides, plus tautologies, stale milestone references and a few
comments that excused hacky code. Comments drift silently, and agents copy the
pattern of an excused hack into new code.

## Decision
- No comments in `.rs`, `.py`, `.sh`, `.yml`/`.yaml` or `.toml` files. Python
  docstrings count as comments.
- A non-obvious decision goes in an ADR under `docs/adr/`. Hacky code gets an
  issue, not a comment.
- User-facing text lives in code: clap help uses `#[arg(help = ...)]` and
  `#[command(about = ...)]`; argparse uses a `DESCRIPTION` constant.
- Not counted (machine-read, not prose): a shebang on line 1, `# noqa`,
  `# type: ignore`, `# shellcheck` directives, and the `# vX.Y.Z` tag after a
  SHA-pinned GitHub Action (Dependabot reads it).
- `scripts/check-comments.py` enforces a per-file ratchet against
  `scripts/comment-baseline.json`. A file may never gain comments. When a file
  loses comments, `python3 scripts/check-comments.py --update` lowers its
  baseline, and the gate fails until that happens. `--update` never raises a
  count. `scripts/accept.sh` runs the ratchet first.

## Remaining baseline
The 27 `// SAFETY:` comments on `unsafe` FFI calls in `src/guarded_fs.rs`,
`src/install_source/git.rs` and `tests/mutations.rs` stay for now. Each one
proves the invariants at a single call site, so an ADR can't replace it.
They're the open question for the next pass: either wrap each call in a small
safe function whose name and types carry the invariant, or make `SAFETY:` the
one sanctioned exemption.

## Consequences
Reasons for code are found in `docs/adr/`, not next to the code. Run
`/delete-comment` (`.agents/skills/delete-comment/`) to repeat the audit.
