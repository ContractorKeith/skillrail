#!/usr/bin/env python3
"""Observable artifact admission, reproducibility and corruption controls."""
import importlib.util
import io
import gzip
import json
import tarfile
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class Artifacts(unittest.TestCase):
    def test_extended_header_refuses_before_payload_allocation(self):
        with tempfile.TemporaryDirectory(prefix="s7-header-") as directory:
            archive = Path(directory) / release.archive_name("0.1.0", release.host_target())
            info = tarfile.TarInfo("extension")
            info.type = tarfile.XHDTYPE
            info.size = 101 * 1024 * 1024
            with gzip.open(archive, "wb") as output:
                output.write(info.tobuf(format=tarfile.USTAR_FORMAT))
                for _ in range(101):
                    output.write(bytes(1024 * 1024))
                output.write(bytes(1024))
            archive.with_suffix(".gz.sha256").write_text(
                f"{release.digest(archive.read_bytes())}  {archive.name}\n")
            probe = '''import sys
if sys.platform == "linux":
    import resource
    resource.setrlimit(resource.RLIMIT_AS, (80 * 1024 * 1024, 80 * 1024 * 1024))
from pathlib import Path
import release
try:
    release.verify(Path(sys.argv[1]))
except ValueError:
    sys.exit(0)
sys.exit(1)
'''
            result = subprocess.run([sys.executable, "-c", probe, str(archive)],
                                    cwd=Path(__file__).parent, capture_output=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr.decode())

    def test_optimized_python_refuses_to_skip_evidence_assertions(self):
        result = subprocess.run([sys.executable, "-O", str(Path(__file__).with_name("release.py")), "--help"],
                                capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)

    def test_build_metadata_and_member_modes_are_validated(self):
        with tempfile.TemporaryDirectory(prefix="s7-metadata-") as directory:
            root = Path(directory)
            binary = root / "binary"
            binary.write_bytes(b"fixture")
            archive = release.package(binary, root / "assets", release.host_target(), "0.1.0", 123)
            original = archive.read_bytes()
            for bad in ("epoch-string", "epoch-bool", "metadata-mode"):
                with self.subTest(bad=bad):
                    with tarfile.open(fileobj=io.BytesIO(original), mode="r:gz") as source:
                        members = source.getmembers()
                        files = {m.name: source.extractfile(m).read() for m in members}
                    meta = json.loads(files["BUILD.json"])
                    if bad != "metadata-mode":
                        meta["source_date_epoch"] = "garbage" if bad == "epoch-string" else True
                    files["BUILD.json"] = json.dumps(meta).encode()
                    files["SHA256SUMS"] = "".join(
                        f"{release.digest(files[n])}  {n}\n" for n in sorted(files) if n != "SHA256SUMS").encode()
                    with tarfile.open(archive, "w:gz", format=tarfile.USTAR_FORMAT) as output:
                        for member in members:
                            member.size = len(files[member.name])
                            if bad == "metadata-mode" and member.name == "BUILD.json":
                                member.mode = 0o777
                            output.addfile(member, io.BytesIO(files[member.name]))
                    archive.with_suffix(".gz.sha256").write_text(
                        f"{release.digest(archive.read_bytes())}  {archive.name}\n")
                    with self.assertRaises(ValueError):
                        release.verify(archive)

    def test_repeatable_archive_and_corruption_refusal(self):
        with tempfile.TemporaryDirectory(prefix="s7-test-") as directory:
            root = Path(directory)
            binary = root / "skillrail"
            binary.write_bytes(b"fixture binary bytes")
            binary.chmod(0o755)
            archive = release.package(binary, root / "a", "x86_64-unknown-linux-gnu", "0.1.0", 123)
            other = release.package(binary, root / "b", "x86_64-unknown-linux-gnu", "0.1.0", 123)
            self.assertEqual(archive.read_bytes(), other.read_bytes())
            metadata = release.verify(archive)
            self.assertEqual(metadata["target"], "x86_64-unknown-linux-gnu")
            archive.write_bytes(archive.read_bytes() + b"tamper")
            with self.assertRaises(ValueError):
                release.verify(archive)

    def test_member_tampering_and_traversal_refuse_with_valid_outer_checksum(self):
        with tempfile.TemporaryDirectory(prefix="s7-test-") as directory:
            root = Path(directory)
            binary = root / "skillrail"
            binary.write_bytes(b"fixture")
            archive = release.package(binary, root / "a", release.host_target(), "0.1.0", 123)
            original = archive.read_bytes()
            for traversal in (False, True):
                with tarfile.open(fileobj=io.BytesIO(original), mode="r:gz") as source:
                    members = [(m, source.extractfile(m).read()) for m in source.getmembers()]
                with tarfile.open(archive, "w:gz") as output:
                    for member, data in members:
                        if member.name == "skillrail":
                            if traversal:
                                member.name = "../skillrail"
                            else:
                                data = b"changed"
                                member.size = len(data)
                        output.addfile(member, io.BytesIO(data))
                archive.with_suffix(".gz.sha256").write_text(f"{release.digest(archive.read_bytes())}  {archive.name}\n")
                with self.assertRaises(ValueError):
                    release.verify(archive)

    def test_wrong_platform_and_version_fail_before_inventory(self):
        with tempfile.TemporaryDirectory(prefix="s7-test-") as directory:
            root = Path(directory)
            binary = root / "skillrail"
            binary.write_text('#!/bin/sh\necho "skillrail 9.9.9"\n')
            binary.chmod(0o755)
            with self.assertRaises(AssertionError):
                release.smoke(binary, "0.1.0")
            foreign = next(t for t in release.TARGETS if t != release.host_target())
            archive = release.package(binary, root / "a", foreign, "0.1.0", 123)
            with self.assertRaises(ValueError):
                release.verify(archive, run=True)

    def test_formula_requires_complete_actual_assets(self):
        with tempfile.TemporaryDirectory(prefix="s7-test-") as directory:
            root = Path(directory)
            binary = root / "skillrail"
            binary.write_bytes(b"fixture")
            release.package(binary, root / "a", release.host_target(), "0.1.0", 123)
            with self.assertRaises(ValueError):
                release.formula(root / "a", "0.1.0")


if __name__ == "__main__":
    unittest.main()
