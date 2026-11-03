#!/usr/bin/env python3
"""Observable artifact admission, reproducibility and corruption controls."""
import importlib.util
import io
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
    def test_optimized_python_refuses_to_skip_evidence_assertions(self):
        result = subprocess.run([sys.executable, "-O", str(Path(__file__).with_name("release.py")), "--help"],
                                capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)

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
