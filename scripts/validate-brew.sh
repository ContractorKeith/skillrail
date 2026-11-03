#!/usr/bin/env bash
# Reversible local tap/install/test against an exact candidate, never publishes.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${TMPDIR:?Set a private disk-backed TMPDIR}"
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_AUTOREMOVE=1
brew_executable=$(command -v brew)
# A timed-out child returns to this shell so its cleanup trap still runs.
brew() {
  python3 - "$brew_executable" "$@" <<'PY_BREW'
import os
import signal
import subprocess
import sys
with subprocess.Popen(sys.argv[1:], stdin=subprocess.DEVNULL, start_new_session=True) as child:
    try:
        status = child.wait(timeout=300)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait()
        status = 124
sys.exit(status)
PY_BREW
}
brew --version
# Refuse existing names; cleanup must never remove someone else's install/tap.
if brew list --versions skillrail >/dev/null 2>&1 || brew tap | rg -qx 'skillrail-check/candidate'; then
  echo 'Existing skillrail install or validation tap: use a clean Homebrew environment.' >&2
  exit 1
fi
fixture=$(mktemp -d "$TMPDIR/b7.XXXX")
tapped=0
installing=0
cleanup() {
  if [ "$installing" = 1 ]; then brew uninstall --force skillrail-check/candidate/skillrail; fi
  if [ "$tapped" = 1 ]; then brew untap skillrail-check/candidate; fi
  rm -rf "$fixture"
}
trap cleanup EXIT
mkdir -p "$fixture/Formula"
python3 scripts/release.py formula --assets "${1:-target/dist}" --output "$fixture/Formula/skillrail.rb" --local-target "$(python3 -c 'import sys; sys.path.insert(0,"scripts"); import release; print(release.host_target())')"
git -c init.defaultBranch=main -c core.hooksPath=/dev/null init --template= -q "$fixture"
git -C "$fixture" add Formula/skillrail.rb
git -C "$fixture" -c user.name=ContractorKeith -c user.email=ContractorKeith@users.noreply.github.com -c core.hooksPath=/dev/null commit -qm 'test: stage local candidate formula'
brew tap skillrail-check/candidate "$fixture"
tapped=1
installing=1
brew install skillrail-check/candidate/skillrail
brew test skillrail-check/candidate/skillrail
python3 - "$(brew --prefix skillrail-check/candidate/skillrail)/bin/skillrail" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, "scripts")
import release
import tomllib
version = tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"]
release.smoke(Path(sys.argv[1]), version)
print("Homebrew installed binary: exact version/help/disposable populated inventory passed")
PY
