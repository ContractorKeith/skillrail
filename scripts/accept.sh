#!/usr/bin/env bash
# Exact local/worker/reviewer/CI gate. Python 3 supplies standard-library PTYs.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
# Includes the official rmcp client initialize/list/every-read-tool integration.
cargo test --locked
cargo build --locked
python3 scripts/test_terminal_attributes.py
python3 scripts/smoke.py
python3 scripts/pty-readiness-smoke.py
python3 scripts/mcp-smoke.py
python3 scripts/m4-smoke.py
python3 scripts/m5-smoke.py
