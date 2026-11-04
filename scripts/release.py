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
import struct
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zlib

if not __debug__:
    raise RuntimeError("Evidence checks require Python assertions; remove -O/PYTHONOPTIMIZE")

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-gnu": ("linux", "on_intel"),
    "aarch64-unknown-linux-gnu": ("linux", "on_arm"),
    "x86_64-apple-darwin": ("macos", "on_intel"),
    "aarch64-apple-darwin": ("macos", "on_arm"),
}
# Expected (format, machine) of the packaged executable: ELF e_machine or Mach-O cputype.
EXECUTABLES = {
    "x86_64-unknown-linux-gnu": ("ELF", 62),
    "aarch64-unknown-linux-gnu": ("ELF", 183),
    "x86_64-apple-darwin": ("Mach-O", 0x01000007),
    "aarch64-apple-darwin": ("Mach-O", 0x0100000C),
}
MAX_EPOCH = 0o77777777777


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
    arch = {"arm64": "aarch64", "aarch64": "aarch64", "x86_64": "x86_64"}.get(platform.machine())
    system = {"Linux": "-unknown-linux-gnu", "Darwin": "-apple-darwin"}.get(platform.system())
    if not arch or not system:
        raise ValueError(f"unsupported host {platform.system()} {platform.machine()}")
    return arch + system


def cargo_version():
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def checked_epoch(value):
    """SOURCE_DATE_EPOCH as an int the tar format can hold, or one sentence."""
    if isinstance(value, str) and re.fullmatch(r"[0-9]{1,11}", value):
        value = int(value)
    if type(value) is not int or not 0 <= value <= MAX_EPOCH:
        raise ValueError(f"SOURCE_DATE_EPOCH must be a whole number of seconds from 0 to {MAX_EPOCH}, got {value!r}")
    return value


def check_tmpdir():
    """tempfile silently falls back to /tmp when TMPDIR is wrong; refuse instead."""
    scratch = os.environ.get("TMPDIR")
    if scratch and not Path(scratch).is_dir():
        raise ValueError(f"TMPDIR {scratch} is not an existing directory; create it first")


def executable_kind(data):
    """(format, machine) for 64-bit little-endian ELF and Mach-O headers, else None."""
    if data[:4] == b"\x7fELF" and len(data) >= 20 and data[4] == 2 and data[5] == 1:
        return "ELF", struct.unpack_from("<H", data, 18)[0]
    if data[:4] == b"\xcf\xfa\xed\xfe" and len(data) >= 8:
        return "Mach-O", struct.unpack_from("<I", data, 4)[0]
    return None


def check_executable(data, target):
    kind = executable_kind(data)
    if kind != EXECUTABLES[target]:
        found = next((name for name, expected in EXECUTABLES.items() if expected == kind),
                     "not a recognized 64-bit ELF or Mach-O executable")
        raise ValueError(f"packaged skillrail executable is built for {found}, not {target}")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def archive_name(version, target):
    if target not in TARGETS or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.-]+)?", version):
        raise ValueError("unsupported target or version")
    return f"skillrail-v{version}-{target}.tar.gz"


def smoke(binary, version):
    """Run only against a disposable inventory with its own Git boundary."""
    check_tmpdir()
    reported = command([binary, "version"]).strip()
    assert reported == f"skillrail {version}", f"binary reports {reported!r}, expected 'skillrail {version}'"
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
    epoch = checked_epoch(epoch)
    metadata = {"version": version, "target": target, "source_date_epoch": epoch}
    files = {"skillrail": binary.read_bytes(), "LICENSE": (ROOT / "LICENSE").read_bytes(),
             "BUILD.json": (json.dumps(metadata, sort_keys=True) + "\n").encode()}
    files["SHA256SUMS"] = "".join(f"{digest(data)}  {name}\n" for name, data in sorted(files.items())).encode()
    # No source paths, build environment or wall clock in the gzip/tar envelope.
    buffer = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=buffer, mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            for entry, data in sorted(files.items()):
                info = tarfile.TarInfo(entry)
                info.size, info.mtime = len(data), epoch
                info.mode = 0o755 if entry == "skillrail" else 0o644
                tar.addfile(info, io.BytesIO(data))
    content = buffer.getvalue()
    # Nothing touches the output directory until the whole archive exists; the
    # final names appear by rename, so a failure never leaves a partial artifact.
    output.mkdir(parents=True, exist_ok=True)
    archive = output / name
    sidecar = archive.with_suffix(archive.suffix + ".sha256")
    partials = []
    try:
        for final, data in ((archive, content), (sidecar, f"{digest(content)}  {name}\n".encode())):
            handle, partial = tempfile.mkstemp(dir=output, prefix=f".{final.name}.", suffix=".partial")
            partials.append((Path(partial), final))
            with os.fdopen(handle, "wb") as out:
                out.write(data)
            os.chmod(partial, 0o644)
        for partial, final in partials:
            os.replace(partial, final)
    finally:
        for partial, _ in partials:
            partial.unlink(missing_ok=True)
    return archive


def read_members(zipped, limits):
    """Fixed USTAR records only; TarFile.getmembers would follow PAX/GNU extensions
    before returning a member. Our generator emits plain USTAR."""
    files = {}
    for _ in range(5):
        header = zipped.read(512)
        if header == bytes(512):
            tail = zipped.read(10241)
            if len(tail) < 512 or len(tail) > 10240 or any(tail):
                raise ValueError("invalid archive end padding")
            return files
        if len(header) != 512 or header[257:263] != b"ustar\0":
            raise ValueError("expected a USTAR header")
        try:
            member = tarfile.TarInfo.frombuf(header, "utf-8", "strict")
        except tarfile.HeaderError as error:
            raise ValueError("invalid archive header") from error
        if (member.type != tarfile.REGTYPE or member.name not in limits
                or member.name in files or not 0 <= member.size <= limits[member.name]
                or member.mode != (0o755 if member.name == "skillrail" else 0o644)):
            raise ValueError("unexpected archive member")
        data = zipped.read(member.size)
        padding_size = -member.size % 512
        padding = zipped.read(padding_size)
        if len(data) != member.size or padding != bytes(padding_size):
            raise ValueError("truncated member or invalid padding")
        files[member.name] = data
    raise ValueError("too many archive members")


def verify(archive, run=False, version=None):
    """Verify bounded USTAR records before reading any declared payload.

    The version must be the checkout's Cargo.toml version unless given.
    """
    version = version or cargo_version()
    limits = {"skillrail": 100 * 1024 * 1024, "BUILD.json": 65536,
              "LICENSE": 65536, "SHA256SUMS": 65536}
    sidecar = archive.with_suffix(archive.suffix + ".sha256")
    with archive.open("rb") as raw:
        checksum = hashlib.sha256()
        compressed_size = 0
        while chunk := raw.read(65536):
            compressed_size += len(chunk)
            if compressed_size > 101 * 1024 * 1024:
                raise ValueError("compressed archive exceeds budget")
            checksum.update(chunk)
        expected = f"{checksum.hexdigest()}  {archive.name}\n".encode()
        try:
            with sidecar.open("rb") as handle:
                recorded = handle.read(1025)
        except FileNotFoundError:
            raise ValueError(f"checksum file {sidecar.name} is missing") from None
        if recorded != expected:
            raise ValueError("archive checksum mismatch")
        raw.seek(0)
        with gzip.GzipFile(fileobj=raw, mode="rb") as zipped:
            try:
                files = read_members(zipped, limits)
            except (EOFError, OSError, zlib.error) as error:
                raise ValueError("archive is not a complete gzip file") from error
    if files.keys() != limits.keys():
        raise ValueError("missing archive members")
    if files["SHA256SUMS"].decode() != "".join(f"{digest(files[n])}  {n}\n" for n in sorted(files) if n != "SHA256SUMS"):
        raise ValueError("member checksum mismatch")
    meta = json.loads(files["BUILD.json"])
    if (not isinstance(meta, dict) or set(meta) != {"version", "target", "source_date_epoch"}
            or not isinstance(meta["version"], str) or not isinstance(meta["target"], str)
            or type(meta["source_date_epoch"]) is not int
            or not 0 <= meta["source_date_epoch"] <= MAX_EPOCH):
        raise ValueError("invalid build metadata")
    if archive.name != archive_name(meta["version"], meta["target"]):
        raise ValueError("archive metadata mismatch")
    if meta["version"] != version:
        raise ValueError(f"archive is version {meta['version']}, expected {version}")
    check_executable(files["skillrail"], meta["target"])
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
        meta = verify(archive, version=version)
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


def describe(error):
    """One line for a refusal; the cause is the message, never a stack."""
    if isinstance(error, subprocess.CalledProcessError):
        lines = (error.stderr or "").strip().splitlines()
        detail = f": {lines[-1]}" if lines else ""
        return f"{Path(str(error.cmd[0])).name} exited {error.returncode}{detail}"
    if isinstance(error, OSError) and error.filename:
        return f"{error.filename}: {error.strerror}"
    return " ".join(str(error).split()) or type(error).__name__


def run(args, parser):
    if args.action == "build":
        epoch = os.environ.get("SOURCE_DATE_EPOCH")
        if epoch is not None:
            epoch = checked_epoch(epoch)
        if args.target != host_target():
            parser.error("build on the matching native host; cross-compilation is not execution evidence")
        check_tmpdir()
        version = cargo_version()
        if epoch is None:
            epoch = checked_epoch(command(["git", "show", "-s", "--format=%ct", "HEAD"], cwd=ROOT).strip())
        command(["cargo", "build", "--locked", "--release", "--target", args.target], cwd=ROOT, timeout=1200)
        metadata = json.loads(command(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT))
        target_dir = Path(metadata["target_directory"])
        binary = (target_dir / args.target / "release/skillrail").resolve()
        smoke(binary, version)
        archive = package(binary, args.output, args.target, version, epoch)
        verify(archive, run=True)
        print(json.dumps({"archive": str(archive), "sha256": digest(archive.read_bytes()), "smoke": "passed"}))
    elif args.action == "verify":
        print(json.dumps(verify(args.archive, args.run), sort_keys=True))
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(formula(args.assets, cargo_version(), args.local_target))
        print(args.output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--debug", action="store_true", help="show tracebacks for refusals")
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
    try:
        run(args, parser)
    except (ValueError, AssertionError, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        if args.debug:
            raise
        subject = {"build": f"build {getattr(args, 'target', '')}", "verify": str(getattr(args, "archive", "")),
                   "formula": str(getattr(args, "assets", ""))}[args.action]
        print(f"release.py: {subject}: {describe(error)}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
