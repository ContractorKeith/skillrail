#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/check-comments.py
python3 scripts/test_check_comments.py
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 scripts/test_release.py
python3 scripts/test_brew_validation.py
python3 scripts/test_brew_failures.py
python3 scripts/test_terminal_attributes.py
python3 scripts/smoke.py
python3 scripts/pty-readiness-smoke.py
python3 scripts/mcp-smoke.py
python3 scripts/m4-smoke.py
python3 scripts/m4-smoke.py --read-size 64
python3 scripts/m5-smoke.py
python3 scripts/m5-smoke.py --read-size 64
python3 scripts/m6-smoke.py
