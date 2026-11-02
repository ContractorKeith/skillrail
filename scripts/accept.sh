#!/usr/bin/env bash
# Shared gate. M0 adds the executable fixture smoke checks.
set -eu
cd "$(dirname "$0")/.."
if [ ! -f Cargo.toml ]; then
  echo 'M0 has not landed: no Rust implementation to verify.' >&2
  exit 1
fi
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
