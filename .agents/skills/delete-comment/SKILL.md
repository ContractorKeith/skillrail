---
name: delete-comment
description: Audit every code comment in Skillrail, sort each one as Tautology, Excuse, Stale or Decision Record, delete it, file an issue for each excused hack, move decisions into docs/adr/, and lower the comment ratchet. Use for "/delete-comment", "run the comment audit", "clean up comments", or when scripts/check-comments.py fails.
---

# delete-comment

Policy: docs/adr/0001-no-code-comments.md. The gate is
`python3 scripts/check-comments.py` (run by `scripts/accept.sh`).

## 1. Inventory
`python3 scripts/check-comments.py --list` prints every counted comment with its
`file:line`, including Python docstrings. Read each one together with the code
it sits on. The tool skips shebangs, `noqa`/`type: ignore`/`shellcheck`
directives, SHA-pin `# vX` tags and `// SAFETY:` blocks.

## 2. Classify each comment
- **Tautology**: says what the code, a name or a test assertion already says.
  Delete it.
- **Stale**: describes code, a milestone (M1–M7), an old implementation or a
  file that no longer exists or no longer matches. Check the code and
  `git log -S` before deciding. Delete it.
- **Excuse**: makes a hack acceptable, such as a magic number decoded in a
  comment, a stringly-typed value with an assumed property, brittle
  text-slicing, or "temporary". Delete it and file a GitHub issue (step 4).
- **Decision Record**: a non-obvious rule the code depends on (security,
  platform quirk, native-agent parity, crash/recovery ordering, protocol
  behavior). Move it into the ADR whose topic fits (`docs/adr/README.md`), or
  start the next-numbered ADR. Write it as a rule and point at the file and
  function, never a line number. Drop milestone names.

User-facing text isn't a comment. Clap help becomes `#[arg(help = "...")]` or
`#[command(about = "...")]` with no trailing period, because clap strips it
from doc comments. Argparse docs become a `DESCRIPTION` constant. Capture
every `--help` output before the change and `diff -r` it afterwards. It must
be identical.

`// SAFETY:` blocks directly above `unsafe` are allowed (ADR 0001) and
aren't counted. Leave them alone unless they no longer match the call.

## 3. Delete
Delete whole comment lines. For a trailing comment, delete the comment and the
whitespace before it. Then:
- `cargo fmt`, then `cargo clippy --locked --all-targets -- -D warnings`.
  Removing a comment can unlock lints such as `collapsible_if`, so fix them
  in the code.
- If a function body was only a docstring, it needs `pass`.

## 4. File excuse issues
One issue per hack: `gh issue create --repo ContractorKeith/skillrail --label
refactor`. Write the title as the fix. In the body, name the file and
function, quote the deleted comment, and give the concrete replacement and a
done-when.

## 5. Ratchet and gate
`python3 scripts/check-comments.py --update` lowers the baseline. It never
raises a count. Then run `bash scripts/accept.sh`. Put a per-comment
classification table in the PR body, plus the line count shed
(`git diff --shortstat`) and how many comments remain.
