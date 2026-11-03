#!/usr/bin/env python3
"""Build native release candidates, verify archives, and render asset-bound tap proposals."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import tarfile
import tempfile
import tomllib

if not __debug__:
    raise RuntimeError("Evidence checks require Python assertions; remove -O/PYTHONOPTIMIZE")

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-gnu": ("linux", "on_intel"),
    "aarch64-unknown-linux-gnu": ("linux", "on_arm"),
    "x86_64-apple-darwin": ("macos", "on_intel"),
    "aarch64-apple-darwin": ("macos", "on_arm"),
}


def command(args, **kwargs):
    timeout = kwargs.pop("timeout", 30)
    with subprocess.Popen([str(a) for a in args], stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, text=True, start_new_session=True, **kwargs) as child:
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate()
            raise RuntimeError("release command timed out; process group stopped") from None
        if child.returncode:
            raise subprocess.CalledProcessError(child.returncode, args, stdout, stderr)
        return stdout


def host_target():
    arch = {"arm64": "aarch64", "aarch64": "aarch64", "x86_64": "x86_64"}[platform.machine()]
    return arch + {"Linux": "-unknown-linux-gnu", "Darwin": "-apple-darwin"}[platform.system()]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def archive_name(version, target):
    if target not in TARGETS or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.-]+)?", version):
        raise ValueError("unsupported target or version")
    return f"skillrail-v{version}-{target}.tar.gz"


def smoke(binary, version):
    """Run only against a disposable inventory with its own Git boundary."""
    assert command([binary, "version"]).strip() == f"skillrail {version}", "binary version mismatch"
    assert command([binary, "--version"]).strip() == f"skillrail {version}"
    assert "install" in command([binary, "--help"])
    with tempfile.TemporaryDirectory(prefix="s7-smoke-") as directory:
        root = Path(directory)
        home, project, state = (root / n for n in ("h", "p", "s"))
        for path in (home, project, state, project / ".git"):
            path.mkdir(mode=0o700)
        skill = project / ".agents/skills/release-fixture"
        skill.mkdir(parents=True)
        (skill / "SKILL.md").write_text("---\nname: release-fixture\ndescription: Inert artifact check\n---\nBODY_CANARY_S7\n")
        args = [binary, "--home", home, "--project", project, "--state-dir", state]
        inventory = json.loads(command([*args, "scan", "--json"]))
        assert inventory["schema_version"] == 1
        assert [item["name"] for item in inventory["items"]] == ["release-fixture"]
        assert len(inventory["occurrences"]) == 1
        assert "BODY_CANARY_S7" not in json.dumps(inventory)
        listed = json.loads(command([*args, "list", "--json"]))
        assert listed["items"] == inventory["items"]


def package(binary, output, target, version, epoch):
    """Deterministic envelope for identical binary/license/metadata inputs."""
    name = archive_name(version, target)
    output.mkdir(parents=True, exist_ok=True)
    metadata = {"version": version, "target": target, "source_date_epoch": epoch}
    files = {"skillrail": binary.read_bytes(), "LICENSE": (ROOT / "LICENSE").read_bytes(),
             "BUILD.json": (json.dumps(metadata, sort_keys=True) + "\n").encode()}
    files["SHA256SUMS"] = "".join(f"{digest(data)}  {name}\n" for name, data in sorted(files.items())).encode()
    archive = output / name
    # No source paths, build environment or wall clock in the gzip/tar envelope.
    with archive.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            for entry, data in sorted(files.items()):
                info = tarfile.TarInfo(entry)
                info.size, info.mtime = len(data), epoch
                info.mode = 0o755 if entry == "skillrail" else 0o644
                tar.addfile(info, io.BytesIO(data))
    archive.with_suffix(archive.suffix + ".sha256").write_text(f"{digest(archive.read_bytes())}  {name}\n")
    return archive


def verify(archive, run=False):
    """Check integrity against the adjacent checksum; reject unsafe/extra members."""
    raw = archive.read_bytes()
    expected = f"{digest(raw)}  {archive.name}\n"
    if archive.with_suffix(archive.suffix + ".sha256").read_text() != expected:
        raise ValueError("archive checksum mismatch")
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as tar:
        members = tar.getmembers()
        names = [m.name for m in members]
        if sorted(names) != ["BUILD.json", "LICENSE", "SHA256SUMS", "skillrail"] or any(not m.isfile() or m.size > 100 * 1024 * 1024 for m in members):
            raise ValueError("unexpected archive members")
        files = {m.name: tar.extractfile(m).read() for m in members}
        if files["SHA256SUMS"].decode() != "".join(f"{digest(files[n])}  {n}\n" for n in sorted(files) if n != "SHA256SUMS"):
            raise ValueError("member checksum mismatch")
        if next(m.mode for m in members if m.name == "skillrail") != 0o755:
            raise ValueError("binary is not executable")
    meta = json.loads(files["BUILD.json"])
    if archive.name != archive_name(meta["version"], meta["target"]):
        raise ValueError("archive metadata mismatch")
    if run:
        if meta["target"] != host_target():
            raise ValueError("execution requires matching native platform/architecture")
        with tempfile.TemporaryDirectory(prefix="s7-extract-") as directory:
            binary = Path(directory) / "skillrail"
            binary.write_bytes(files["skillrail"])
            binary.chmod(0o755)
            smoke(binary, meta["version"])
    return meta


def formula(assets, version, local_target=None):
    """Full proposal requires four actual archives. Local proposal tests one host."""
    targets = [local_target] if local_target else list(TARGETS)
    blocks = []
    for target in targets:
        archive = assets / archive_name(version, target)
        if not archive.is_file():
            raise ValueError(f"missing required asset: {archive.name}")
        meta = verify(archive)
        assert meta["target"] == target and meta["version"] == version
        url = archive.resolve().as_uri() if local_target else f"https://github.com/ContractorKeith/skillrail/releases/download/v#{{version}}/{archive_name(version, target).replace(version, '#{version}', 1)}"
        os_name, arch = TARGETS[target]
        blocks.append(f'  on_{os_name} do\n    {arch} do\n      url "{url}"\n      sha256 "{digest(archive.read_bytes())}"\n    end\n  end\n')
    return '''# frozen_string_literal: true
# Generated from verified candidate archives; publication is a separate gate.
class Skillrail < Formula
  desc "Local-first skill and MCP registry for Claude Code and Codex"
  homepage "https://github.com/ContractorKeith/skillrail"
  version "''' + version + '''"
  license "MIT"

''' + "\n".join(blocks) + '''
  def install
    bin.install "skillrail"
    prefix.install "LICENSE", "BUILD.json", "SHA256SUMS"
  end

  test do
    assert_equal "skillrail #{version}", shell_output("#{bin}/skillrail version").strip
    assert_match "install", shell_output("#{bin}/skillrail --help")
    (testpath/"home").mkpath
    (testpath/"project/.git").mkpath
    output = shell_output("#{bin}/skillrail --home #{testpath}/home --project #{testpath}/project --state-dir #{testpath}/state scan --json")
    assert_equal [], JSON.parse(output).fetch("items")
  end
end
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    build = commands.add_parser("build")
    build.add_argument("--target", choices=TARGETS, required=True)
    build.add_argument("--output", type=Path, default=ROOT / "target/dist")
    check = commands.add_parser("verify")
    check.add_argument("archive", type=Path)
    check.add_argument("--run", action="store_true")
    render = commands.add_parser("formula")
    render.add_argument("--assets", type=Path, required=True)
    render.add_argument("--output", type=Path, required=True)
    render.add_argument("--local-target", choices=TARGETS)
    args = parser.parse_args()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    if args.action == "build":
        if args.target != host_target():
            parser.error("build on the matching native host; cross-compilation is not execution evidence")
        command(["cargo", "build", "--locked", "--release", "--target", args.target], cwd=ROOT, timeout=1200)
        target_dir = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        if not target_dir.is_absolute():
            target_dir = ROOT / target_dir
        binary = (target_dir / args.target / "release/skillrail").resolve()
        smoke(binary, version)
        epoch = int(os.environ.get("SOURCE_DATE_EPOCH", command(["git", "show", "-s", "--format=%ct", "HEAD"], cwd=ROOT).strip()))
        archive = package(binary, args.output, args.target, version, epoch)
        verify(archive, run=True)
        print(json.dumps({"archive": str(archive), "sha256": digest(archive.read_bytes()), "smoke": "passed"}))
    elif args.action == "verify":
        print(json.dumps(verify(args.archive, args.run), sort_keys=True))
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(formula(args.assets, version, args.local_target))
        print(args.output)


if __name__ == "__main__":
    main()
