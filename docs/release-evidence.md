# M7 candidate evidence ledger

Observed on 2026-11-03 in the T3-managed M7 worktree, starting at
`12765e6326f34f0f7038717ba36f86e26b0a8872`. This is author evidence, not the
parent's independent review, adversarial QA or release skeptic. Source head/tree
are recorded in the draft PR/handoff; rebuilding a different tree requires new
asset checksums and formula validation. Historical M1–M6 evidence remains in
compatibility.md; the parent's supplied M6 Linux/macOS results are not relabeled
as M7 native execution.

## Machine and exact executable observations

Linux x86_64, kernel `7.2.8-9-omarchy`, Omarchy, glibc **2.44**,
Rust **1.98.1 (48a229cea 2026-09-01)**, GCC **16.2.1 20260810**,
Git **2.56.0**, Python **3.15.2**, Homebrew **7.0.12-4-g856ce8c**.
Disk-backed private scratch: `/home/contractorkeith/.cache/s7`; /tmp is shared
tmpfs and was not used as the harness fixture root. No HOME/CODEX_HOME overrides,
credential copies, real agent-config/skill inventory or other-machine commands.

- Exact native `/home/contractorkeith/.local/share/mise/installs/claude/2.1.316/claude --version`:
  `2.1.316 (Claude Code)`. Executable SHA-256
  `fd13c70000fec3f2116dfbcbb6121210b4a29328bf0e46210a59dd594a67d824`.
- Exact native `/home/contractorkeith/.local/share/mise/installs/codex/0.166.0/bin/codex --version`:
  `codex-cli 0.166.0`. `--help` inspected, no session/config command started.
- Packaged, extracted, source-installed and Homebrew-installed Skillrail binaries
  each returned `skillrail 0.1.0`; metadata alone was not used for this assertion.

The first bare `claude --version` resolved to a mise updater wrapper and attempted
a download. It was interrupted; all subsequent Claude checks used the exact
installed executable. No native version upgrade is part of this change.

## Rerunnable commands and observed scope

Use `TMPDIR=/home/contractorkeith/.cache/s7` for every command below.

| Command | Observed result |
|---|---|
| `bash scripts/accept.sh` | 166 Rust tests, 3 existing terminal Python tests, 5 archive tests, 1 cleanup regression; fmt/strict Clippy/build and all inherited CLI/cache/MCP/M4/M5/M6 PTY/recovery smokes pass |
| `cargo run --locked -- --help` | exit 0, final command surface |
| `git diff --check` and `git diff --check 12765e6...HEAD` | pass; final branch checked at handoff |
| `python3 scripts/release.py build --target x86_64-unknown-linux-gnu` | optimized build; exact version/help, archive/member checksums, extraction and isolated populated inventory pass |
| `python3 scripts/release.py verify target/dist/skillrail-v0.1.0-x86_64-unknown-linux-gnu.tar.gz --run` | matching native Linux execution passes |
| `python3 scripts/native-release-evidence.py claude --claude-binary /home/contractorkeith/.local/share/mise/installs/claude/2.1.316/claude` | real local plugin installation in user/project/local scopes, v2 native records and exactly three selected replay occurrences pass; no model session |
| `python3 scripts/native-release-evidence.py https` | real anonymous HTTPS acquisition/apply of pinned public inert template, exact bytes and payload exclusion pass |
| `bash scripts/validate-brew.sh target/dist` | local checksum-bound formula installs, brew test plus exact binary/populated fixture smoke pass; final cleanup preserves unrelated packages |
| `cargo install --locked --path . --target x86_64-unknown-linux-gnu --root /home/contractorkeith/.cache/s7/source-install` | installs source candidate; exact installed binary passes version/help/populated inventory |

Native/HTTPS child limits are 45/120 seconds, build 1200, executable smokes 30,
Homebrew subprocesses 300, and CI jobs 45 minutes. Timeout kills the process group
for the bounded child. Existing MCP stdin handling, PTY draining, complete-draw
markers, restoration checks and deadlines are unchanged.

Artifact directory: `target/dist/`. The locally tested archive SHA-256 is
`11c3011c036a97365d6a2a46d8e151bdace62964ccaa588d43805fb34b4eae29`,
with SOURCE_DATE_EPOCH **1793710815** (the supplied base commit time).
Repeating packaging before commit produced the same bytes. Preserve that epoch
explicitly to reproduce this envelope; default builds after a new commit use
that commit's time and therefore can have a different archive hash. Local
binaries were linked/executed against glibc 2.44; older-glibc operation was not
inferred. CI builds on Ubuntu 24.04 will be separate exact assets/evidence.
The four-target workflow has not been run locally, and no Mac/cross execution
is claimed. `BUILD.json` identifies target/version/epoch, not a verified source
SHA; correlate the workflow run's source SHA with its artifacts.

Generated one-host local formula: `target/tap/skillrail.rb` (recreate with
`python3 scripts/release.py formula --assets target/dist --output target/tap/skillrail.rb --local-target x86_64-unknown-linux-gnu`).
The full four-target template lives in `scripts/release.py:formula`; see
packaging/README.md. Rendering it without three missing native assets refuses
explicitly. No unproduced hash is represented as verified.

Local raw command logs are outside the checkout under the scratch root:
`accept.log`, `help.log`, `package.log`, `claude.log`, `https.log`, `brew.log`,
`source-install.log`, `version-red.log`, `brew-red.log`. Native stdout/stderr
is withheld by the harness; native logs contain only allowlisted summary facts,
commands with normalized fixture paths and executable hashes. Never attach raw
agent configuration, authentication or private journals to a PR.

## Official isolation and real HTTPS evidence

[Claude environment variables](https://code.claude.com/docs/en/env-vars)
document CLAUDE_CONFIG_DIR for settings/history/plugins and update/telemetry
controls. The harness allows only a minimal per-process environment, generates
its own marketplace/skill, and installs with these native commands:

```text
claude plugin marketplace add FIXTURE/market --json
claude plugin install inert@s7-fixture --scope user --json
claude plugin install inert@s7-fixture --scope project --json
claude plugin install inert@s7-fixture --scope local --json
```

HOME and CODEX_HOME remain inherited and unchanged. No provider auth is supplied.
The fixture is deleted afterwards. This verifies native configuration records,
not model-session skills/toggles or MCP loading.

The [official OpenAI environment reference](https://learn.chatgpt.com/docs/config-file/environment-variables)
defines CODEX_HOME as the config/auth/log/session/skills root.
The [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
and actual 0.166.0 help establish `-c` value overrides, not an allowed replacement
for all home roots. No safe verified root flag under this task's no-HOME/
CODEX_HOME-override rule was found. A real Codex session was not started against
the personal account. Parent prerequisite: disposable OS account/environment
with its own default home, no personal auth/config/skills, then safe independent
authentication if necessary. Both agents' live skill loading, toggle effects and
MCP sessions on Linux/macOS remain pending.

Anonymous HTTPS source:
`https://github.com/anthropics/skills.git`, commit
`ca45fc496a3436251f191354091d557da31c15b5`, subpath `template`.
Only the inert template's SKILL.md is installed; expected byte SHA-256
`eb685d91de039ed864fbd790cddf31684b017fd4a34ee1a55760d8d7cdbadefa`.
The harness uses explicit home/project/state, `--git --ref <commit> --skill template
--agent codex --scope project --apply --json`, verifies exact snapshot bytes and
one inventory item and exact recorded revision/subpath, and excludes body text from public output. Existing helpers,
askpass, config, redirects/proxy and implicit-netrc guards were not relaxed.
The first invocation omitted `--git` and correctly exited 2; adding the required
CLI flag corrected the probe, not the application. macOS HTTPS remains pending.

## Failing-first proof and cleanup correction

- New archive tests initially failed because the implementation file did not yet
  exist. This is feature scaffolding evidence, not a pre-existing product bug.
  Five tests now pass: repeated envelopes/corruption, internal tampering/traversal
  with a valid outer checksum, wrong version/foreign execution, missing formula
  assets, plus optimized-Python refusal so evidence assertions cannot be disabled. No documentation/status prose assertions were added.
- After the version metadata change, `cargo test --locked --test cli` failed:
  actual `skillrail 0.1.0`, old expected `skillrail 0.1.0-dev` (8 passed, 1 failed).
  The public binary test now expects the package's exact version; the full gate
  passes. No Rust runtime behavior was changed.
- Initial Homebrew validation passed install/tests, but its default uninstall
  autoremoved pre-existing `icu4c@78 78.3_1` and `python@3.14 3.14.10`.
  Both exact versions were restored and verified with `brew list --versions`.
  The validator now exports HOMEBREW_NO_AUTOREMOVE=1 and
  HOMEBREW_NO_INSTALL_CLEANUP=1. A fake package-manager public-boundary regression
  (real installed-binary smoke, inert archive envelope) failed before the fix
  with `validator cleanup autoremoved unrelated package`, then passed. An actual
  Homebrew rerun passed installation/tests and removed only its own candidate
  and local tap. Earlier fixture-size errors were test setup errors, not red
  product evidence. Homebrew also refreshed its own Ruby tooling prerequisites;
  no application dependency was added.

## Remaining parent gates

The first Intel macOS candidate job at `ac63a05` failed before packaging in
`m5-smoke.py`'s immediate undo-preview assertion. Reading the actual PTY in
64-byte fragments reproduced that assertion on Linux. M5 now uses M4's pinned
backend completed-draw boundary and requires a newer draw after each key;
the gate exercises both normal and fragmented reads. Ten-second deadlines,
preview/undo, source preservation, terminal restoration and canary assertions
remain unchanged. This is test synchronization, not a TUI runtime modification;
new Intel macOS CI must confirm the correction.

Parent QA correction: fake Homebrew failure probes reproduced failed discovery
proceeding into mutation, partial tap creation being abandoned, and cleanup
overwriting the original error. Three regression tests (ten failure/existing-state
scenarios) now exercise fail-closed discovery, owned partial-tap cleanup,
independently safe later cleanup, and explicit retained-state recovery. The
initial run failed eight assertions; the corrected validator passes. These are
fake-boundary results, not another actual Homebrew install. The existing cleanup
test now resolves its executable through Cargo metadata; its external-target
run passed. No real Homebrew mutations were used for these corrections.

| Gate | State |
|---|---|
| Linux x86_64 local acceptance/artifact/source install/local Homebrew/HTTPS | Observed above |
| Linux arm64 acceptance/archive/extraction/Homebrew/native sessions | Pending native CI/environment |
| macOS x86_64 and arm64 acceptance/archive/extraction/Homebrew/HTTPS/native sessions | Pending native CI/environment |
| Claude/Codex actual disposable session skill/toggle/MCP verification | Pending safe isolation/auth prerequisites |
| Full four-asset formula and audit; final HTTPS formula installs | Pending other native assets, then actual published URLs |
| Independent Standards/Spec/adversarial reviews and fresh release skeptic | Parent responsibility; author self-reviews are separate |
| Tag/release/tap publication and post-tag downloaded-asset checks | Parent only; not performed |

No PLAN checkbox is marked passed by these pending prerequisites. See RELEASE.md
for the pre-tag versus mandatory post-tag ordering.

The installed-source smoke can be repeated after the cargo install command:

```sh
TMPDIR=/home/contractorkeith/.cache/s7 python3 - <<'PY_SOURCE'
import sys
from pathlib import Path
sys.path.insert(0, "scripts")
import release
release.smoke(Path("/home/contractorkeith/.cache/s7/source-install/bin/skillrail"), "0.1.0")
PY_SOURCE
```
