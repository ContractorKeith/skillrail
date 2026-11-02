#!/usr/bin/env bash
# Exact local/worker/reviewer/CI gate. Python 3 supplies standard-library PTYs.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 scripts/smoke.py
