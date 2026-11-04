# Homebrew proposal

`../scripts/release.py` contains the formula template. It follows the existing
ContractorKeith/homebrew-tap `Formula/mdkb.rb` convention (read-only inspection
2026-11-03): `on_macos`/`on_linux`, `on_arm`/`on_intel`, versioned GitHub URLs,
per-asset SHA-256, `bin.install`, and an executable test. No tap is published here.

Generate the complete proposed `Formula/skillrail.rb` only after collecting the
four exact candidate archives and adjacent `.sha256` files into `target/dist`:

```sh
python3 scripts/release.py formula --assets target/dist --output target/proposed/Formula/skillrail.rb
```

The generator verifies every archive/member checksum and requires all four
platforms. There are no placeholder hashes. It does not claim that candidate
assets have been published. Missing artifacts fail explicitly. Publish these
exact bytes after the release gates, or regenerate/revalidate the formula if
any asset changes.

Before tag, validate a host-specific local-file variant of the same template:

```sh
export TMPDIR="$HOME/.cache/skillrail-release/tmp"; mkdir -p "$TMPDIR"   # must exist and be disk-backed
bash scripts/validate-brew.sh target/dist
```

Requires Homebrew, Python >=3.11, Git, a clean `skillrail` install name and
`skillrail-check/candidate` tap name. It creates a temporary local Git tap,
installs the archive with its actual SHA-256, runs `brew test` and a populated
isolated inventory, then uninstalls and untaps. Discovery errors refuse before
mutation. A partially created tap is cleaned only when its local Git origin
matches this run's source. Every brew subprocess has a 300-second timeout
(`SKILLRAIL_BREW_TIMEOUT` seconds overrides it for tests); a timeout returns to the cleanup trap. Cleanup preserves the original failure
and attempts each independently safe step. If an install remains, state cannot
be queried, ownership differs, or untap fails, the tap/source are retained with
inspection and recovery instructions. Cleanup is never silently reported as
success. Autoremove and install cleanup are disabled
to preserve unrelated packages. It never publishes a repository.
Homebrew may download its own tooling prerequisites. Do not run concurrent
Homebrew changes during validation. The generated one-host formula is a test
artifact; it is not the four-target tap proposal. A parent must rerun validation
on each native target and later against actual published HTTPS asset URLs.
See [release checklist](../docs/RELEASE.md) for the non-circular release order.
