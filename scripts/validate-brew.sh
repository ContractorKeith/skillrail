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
        status = child.wait(timeout=float(os.environ.get("SKILLRAIL_BREW_TIMEOUT", "300")))
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait()
        status = 124
sys.exit(status)
PY_BREW
}
brew --version
# Refuse existing names; cleanup must never remove someone else's install/tap.
installed=$(brew list --formula --versions) || { echo 'Cannot establish installed formula state; refusing changes.' >&2; exit 1; }
taps=$(brew tap) || { echo 'Cannot establish tap state; refusing changes.' >&2; exit 1; }
has_candidate() {
  while read -r name rest; do
    case "$name" in skillrail|*/skillrail) return 0 ;; esac
  done <<< "$1"
  return 1
}
if has_candidate "$installed" || [[ $'\n'"$taps"$'\n' == *$'\nskillrail-check/candidate\n'* ]]; then
  echo 'Existing skillrail install or validation tap: use a clean Homebrew environment.' >&2
  exit 1
fi
tap_repo=$(brew --repo skillrail-check/candidate) || exit 1
if [ -z "$tap_repo" ] || [ -e "$tap_repo" ] || [ -L "$tap_repo" ]; then
  echo 'Candidate tap path is unavailable or already exists; refusing changes.' >&2
  exit 1
fi
fixture=$(mktemp -d "$TMPDIR/b7.XXXX")
tapped=0
installing=0
cleanup() {
  status=$?
  trap - EXIT
  cleanup_failed=0
  retain=0
  if [ "$tapped" = 1 ] && { [ -e "$tap_repo" ] || [ -L "$tap_repo" ]; }; then
    # A failed tap command may still create a checkout. Remove only our origin.
    origin=$(git -C "$tap_repo" config --local --get remote.origin.url) || origin=
    if [ "$origin" != "$fixture" ] || [ -L "$tap_repo" ]; then
      echo 'Candidate tap ownership is uncertain; retained for inspection.' >&2
      retain=1
      cleanup_failed=1
    else
      if [ "$installing" = 1 ]; then
        brew uninstall --force skillrail-check/candidate/skillrail || cleanup_failed=1
      fi
      if remaining=$(brew list --formula --versions); then
        if has_candidate "$remaining"; then
          echo 'Candidate remains installed; its tap and source are retained.' >&2
          retain=1
          cleanup_failed=1
        elif ! brew untap skillrail-check/candidate; then
          echo 'Candidate untap failed; source retained.' >&2
          retain=1
          cleanup_failed=1
        fi
      else
        echo 'Cannot establish cleanup state; candidate tap/source retained.' >&2
        retain=1
        cleanup_failed=1
      fi
    fi
  elif [ "$installing" = 1 ]; then
    echo 'Candidate tap disappeared during validation; source retained.' >&2
    retain=1
    cleanup_failed=1
  fi
  if [ "$retain" = 0 ]; then
    rm -r -- "$fixture" || cleanup_failed=1
  else
    printf 'Inspect candidate state before retrying cleanup: tap=%s source=%s\n' "$tap_repo" "$fixture" >&2
    echo 'After inspection: brew uninstall skillrail-check/candidate/skillrail; brew untap skillrail-check/candidate. Preserve unrelated state.' >&2
  fi
  if [ "$status" = 0 ] && [ "$cleanup_failed" != 0 ]; then status=1; fi
  exit "$status"
}
trap cleanup EXIT
mkdir -p "$fixture/Formula"
python3 scripts/release.py formula --assets "${1:-target/dist}" --output "$fixture/Formula/skillrail.rb" --local-target "$(python3 -c 'import sys; sys.path.insert(0,"scripts"); import release; print(release.host_target())')"
git -c init.defaultBranch=main -c core.hooksPath=/dev/null init --template= -q "$fixture"
git -C "$fixture" add Formula/skillrail.rb
git -C "$fixture" -c user.name=ContractorKeith -c user.email=ContractorKeith@users.noreply.github.com -c core.hooksPath=/dev/null commit -qm 'test: stage local candidate formula'
tapped=1
brew tap skillrail-check/candidate "$fixture"
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
