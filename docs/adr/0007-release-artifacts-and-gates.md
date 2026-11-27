# 0007: Release artifacts, gates and CI boundaries

Status: accepted (recorded from code comments, 2026-11-27)

## Decisions
- `scripts/accept.sh` is the exact gate for local runs, workers, reviewers and
  CI. Python 3 is required because its standard library provides PTYs.
- `.github/workflows/artifacts.yml` builds candidate archives only. It has no
  tag trigger, no write permission and no publication step. The release owner
  publishes by hand (docs/RELEASE.md).
- Archives are deterministic for identical binary, license and metadata
  inputs. No source paths, build environment or wall clock enter the gzip/tar
  envelope (`mtime=0`, `SOURCE_DATE_EPOCH`).
- Nothing touches the output directory until the whole archive exists. Final
  names appear by rename, so a failure never leaves a partial artifact.
- `tempfile` silently falls back to `/tmp` when `TMPDIR` is wrong, so the
  release script refuses instead.
- Verification reads fixed USTAR records only, and checks them before reading
  any declared payload. `TarFile.getmembers` would follow PAX/GNU extensions
  before returning a member. The generator emits only plain USTAR. The version
  defaults to the checkout's `Cargo.toml` version.
- A full tap proposal needs all four real archives. A local proposal tests one
  host. A refusal is one line naming the file and the problem, never a stack
  trace.
- Release smokes run only against a disposable inventory with its own Git
  boundary.
- `scripts/validate-brew.sh` is a reversible local tap/install/test against an
  exact candidate and never publishes. It refuses if a `skillrail` install or
  the validation tap already exists, so cleanup can't remove someone else's.
  A failed tap can still leave a checkout behind, and only a checkout whose
  origin is our fixture gets removed. Timed-out `brew` children are killed by
  process group and return to the shell, so the cleanup trap still runs.
