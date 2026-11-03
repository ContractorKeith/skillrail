# v0.1.0 release procedure

This branch prepares a release candidate. The parent owns independent reviews,
adversarial QA, native/platform prerequisites, release skeptic, tag, publication
and tap integration. No workflow here creates a release or tag. AGENTS.md and
the canonical PLAN acceptance checklist remain unchanged.

## Prerequisites and four native builds

Rust 1.98.1 with rustfmt/Clippy, a C compiler/linker (bundled SQLite), Python
>=3.11, and system Git (`/usr/bin/git` for acquisition). No new Rust/Python
package is needed. Evidence commands refuse Python -O/PYTHONOPTIMIZE so checks
cannot be silently disabled. Locked dependencies and a pinned Rust toolchain constrain
inputs. CI selects Python 3.13.9 for tomllib availability; the Python setup action and
GitHub checkout/upload actions are pinned to full upstream SHAs; the
upload action only stores run-scoped candidate artifacts, never a
publication mechanism. PR artifacts are untrusted until their source SHA and
checksums have been reviewed. Never execute an untrusted PR artifact on a
credential-bearing machine.

| Native runner | Rust target | Runtime limit |
|---|---|---|
| Ubuntu 24.04 x86_64 | x86_64-unknown-linux-gnu | glibc 2.39 build host; older glibc unverified |
| Ubuntu 24.04 arm64 | aarch64-unknown-linux-gnu | glibc 2.39 build host; older glibc unverified |
| macOS 15 Intel | x86_64-apple-darwin | matching runner OS verified only after its job passes |
| macOS 14 arm64 | aarch64-apple-darwin | matching runner OS verified only after its job passes |

These are workflow targets, not a claim that all jobs passed. Linux uses GNU
libc, not musl; no Windows binaries, universal Mach-O, signing/notarization or
self-updater are supplied. Inspect native `ldd`/`otool -L` before widening OS
claims. CI always runs the same `bash scripts/accept.sh`, including all inherited
Rust/Python/CLI/MCP/PTY/recovery gates. The separate candidate matrix runs it on
all four hosts before packaging. The 45-minute job limit bounds CI. A missing
host/toolchain/action is a failed or pending prerequisite, not a skip.

From each matching native host, with a short private disk-backed TMPDIR:

```sh
TMPDIR=/home/contractorkeith/.cache/s7 bash scripts/accept.sh
TMPDIR=/home/contractorkeith/.cache/s7 cargo run --locked -- --help
git diff --check
git diff --check 12765e6...HEAD
TMPDIR=/home/contractorkeith/.cache/s7 python3 scripts/release.py build --target x86_64-unknown-linux-gnu
TMPDIR=/home/contractorkeith/.cache/s7 python3 scripts/release.py verify target/dist/skillrail-v0.1.0-x86_64-unknown-linux-gnu.tar.gz --run
```

Use a local private disk path on macOS and substitute the matching target.
`build` refuses a foreign host, uses `cargo build --locked --release --target`
with a 1200-second timeout, checks exact binary `version`/`--version`/help,
packages, verifies, extracts and runs a populated disposable inventory. Each
version/help/inventory subprocess has a 30-second timeout. Foreign archives can
be checksum-verified without `--run`; that is not execution evidence.

Archives are `skillrail-v0.1.0-<rust-target>.tar.gz`, with flat regular members
`skillrail` (0755), `LICENSE`, `BUILD.json`, and `SHA256SUMS` (0644). The adjacent
`.tar.gz.sha256` checksums the complete archive. BUILD records version, target
and SOURCE_DATE_EPOCH (default source commit time). The tar/gzip envelope has
fixed ordering, UID/GID, modes, timestamps and no host path or gzip filename.
Identical binary/license/metadata inputs produce identical archives; this is
**not** a claim of bit-identical Rust binaries across hosts, toolchains or build
paths. Record source head/tree, rustc and compiler versions with each build. Native
runner labels are listed in the [GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Checksums detect corruption; authenticity depends on trusted delivery/review.

`Candidate archives` runs on PRs or workflow_dispatch. Download all four artifacts
from the same reviewed source SHA into `target/dist` (retain sidecars). Workflow
checkout of a PR normally tests its synthetic merge commit; record that SHA,
not just the branch name. Rebuild if the chosen release tree changes. Do not
substitute one architecture's binary or use cross-compilation as native proof.

## Native and network evidence

These optional probes are deliberately outside offline acceptance and CI:

```sh
TMPDIR=/home/contractorkeith/.cache/s7 python3 scripts/native-release-evidence.py claude --claude-binary /home/contractorkeith/.local/share/mise/installs/claude/2.1.316/claude
TMPDIR=/home/contractorkeith/.cache/s7 python3 scripts/native-release-evidence.py https
```

`--binary /absolute/candidate/skillrail` selects an extracted binary; default is
target/debug/skillrail. Claude requires the exact expected version (default
2.1.316; an intentional new native version needs `--expected-claude-version`).
Use an exact executable, never an updater/version-manager wrapper. Every native
CLI command is bounded at 45 seconds, HTTPS acquisition at 120; timeout stops
the child process group. The harness suppresses all raw native output and only
emits allowlisted facts/fixture commands. Each fixture uses private disposable
home/project/state and its own Git boundary outside the checkout. Native env
allows only inherited HOME/CODEX_HOME/TMPDIR, fixed PATH/LANG, isolated Git and
Claude config overrides. Neither HOME nor CODEX_HOME is assigned a new value;
no real auth/config/skills are copied. No model session or MCP endpoint is started.

Claude's documented CLAUDE_CONFIG_DIR isolates settings/history/plugins. The
probe installs an inert generated plugin in user/project/local scope and checks
Skillrail's native-record replay. It does **not** prove a running session loaded
skills, native toggle effects, or MCP tools. Codex 0.166.0's documented root
is CODEX_HOME; `-c` overrides config values, not every config/auth/skill root.
No verified allowed config-root flag is available under this task's prohibition
on HOME/CODEX_HOME overrides. Only exact-binary version/help were run for Codex.
The parent must arrange a genuinely disposable OS environment/account and
separate safe authentication if needed, then record native skill/toggle/MCP
session outcomes on Linux/macOS. Never infer a pass from the pinned-source probes.

HTTPS uses the pinned public inert `anthropics/skills` template commit/subpath
in the harness. It exercises actual Skillrail anonymous HTTPS install and exact
snapshot bytes without executing content. Existing Git credential guards remain
in force: env cleared by the application, helpers/askpass/global/system config,
redirects/proxies disabled; implicit OS-account netrc metadata checked without
reading values. A missing network/TLS/netrc prerequisite must fail or remain
pending. Do not disable the guards to obtain a pass.

## Formula, source install and publication sequence

1. Pre-tag: pass all native acceptance/artifact smokes and disposable native
   session gates; record exact SHAs/versions/commands in the evidence ledger.
2. Pre-tag: collect four actual archives/sidecars and generate the full formula:
   `python3 scripts/release.py formula --assets target/dist --output target/proposed/Formula/skillrail.rb`.
   Missing assets or mismatched hashes refuse. Validate the host-local variant
   with `TMPDIR=/home/contractorkeith/.cache/s7 bash scripts/validate-brew.sh target/dist`.
   It uses a temporary local tap, actual file URL/hash and executable tests,
   then removes its install/tap, with autoremove/cleanup disabled and each brew
   subprocess bounded at 300 seconds. The full proposal uses future versioned HTTPS
   URLs; it does not claim they already exist. See packaging/README.md.
3. Pre-tag: independent full-branch Standards, Spec and adversarial reviews,
   plus a fresh release skeptic. Fix confirmed findings and repeat affected/full
   gates. Parent decides the release is eligible; the worker cannot certify it.
4. Parent only: tag the reviewed tree and publish those exact native archives
   and checksums. Do not silently rebuild different bytes after formula testing.
5. Mandatory post-tag check: download every published archive and sidecar from
   the actual versioned release URLs; compare to reviewed candidate checksums,
   run `release.py verify --run` on each native host, then test/install the final
   generated HTTPS formula on every supported host. Run `brew audit --strict`
   and `brew test` with current tap tooling; resolve real findings.
6. Parent only: integrate/publish the tap formula, verify
   `brew install ContractorKeith/tap/skillrail` from a clean environment and
   `skillrail version` == `skillrail 0.1.0`. Record published URLs/checksums and
   post-tag evidence. Close remaining gates only on observed results.

Post-tag URL checks cannot be prerequisites for creating a tag that makes those
URLs exist. They remain mandatory publication verification, clearly separate
from pre-tag local artifact proof. No hash placeholder is a verified asset.

Source install from the reviewed checkout (no crates.io publication implied):

```sh
cargo install --locked --path .
skillrail version
```

After publication only: `cargo install --locked --git https://github.com/ContractorKeith/skillrail --tag v0.1.0`.
For isolated validation use `--root /private/disposable/install`; execute that
root's actual binary for version/help and explicit fixture inventory. The version
in Cargo metadata alone is not executable evidence.

## Recovery and contract handoff

Public inventory/query JSON stays schema 1, with additive fields and revision-
bound pagination. Mutation plans/history remain their documented envelopes;
private journal schema 4 retains schemas 1/2/3 recovery. Stdio MCP negotiates the
four versions in docs/CLI.md and exposes exactly five read-only tools; no HTTP
server, skill bodies, secrets or mutation MCP tools. Inventory never performs
network discovery or starts configured servers.

Stop mutations after an interrupted prepared/undoing record. With the original
home/project/state, run `operations --json`, preview `undo <operation-id> --json`,
then apply undo only after reviewing its preconditions. Preserve the private
journal and retained originals/stages together. Conflicts preserve intervening
edits; there is no force overwrite, automatic forward replay, cascading undo
stack or backup pruning. Only index.sqlite3 is rebuildable. See the detailed
[CLI recovery contract](CLI.md#durable-journal-history-and-recovery) and
[lifecycle limits](CLI.md#owned-update-and-uninstall). Same-user final-check
races, external references outside selected inventory and filesystem durability
limits remain explicit; packaging does not strengthen these guarantees.
