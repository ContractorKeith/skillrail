#!/usr/bin/env python3
"""Observable artifact admission, reproducibility and corruption controls."""
import importlib.util
import io
import gzip
import json
import os
import shutil
import tarfile
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_fixtures import stub_binary  # noqa: E402

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class Artifacts(unittest.TestCase):
    def test_build_packages_current_cargo_configured_artifact(self):
        # Real Cargo, a dependency-free fixture, and a stale default executable.
        for setting in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR", "config-file"):
            with self.subTest(setting=setting), tempfile.TemporaryDirectory(prefix="s7-build-") as directory:
                root = Path(directory)
                (root / "scripts").mkdir()
                (root / "src").mkdir()
                shutil.copyfile(release.__file__, root / "scripts/release.py")
                shutil.copyfile(release.ROOT / "LICENSE", root / "LICENSE")
                (root / "Cargo.toml").write_text(
                    '[package]\nname="skillrail"\nversion="0.1.0"\nedition="2024"\n')
                (root / "src/main.rs").write_text('''fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--json") {
        println!("{}", r#"{"schema_version":1,"items":[{"name":"release-fixture"}],"occurrences":[{}]}"#);
    } else if args.iter().any(|a| a == "--help") {
        println!("install");
    } else { println!("skillrail 0.1.0"); }
}
''')
                env = os.environ.copy()
                env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
                for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"):
                    env.pop(key, None)
                if setting == "config-file":
                    (root / ".cargo").mkdir()
                    (root / ".cargo/config.toml").write_text('[build]\ntarget-dir="configured"\n')
                else:
                    env[setting] = str(root / "configured")
                stale = root / "target" / release.host_target() / "release/skillrail"
                stale.parent.mkdir(parents=True)
                stale.write_text('#!/bin/sh\necho "skillrail 9.9.9"\n')
                stale.chmod(0o755)
                for command in (["git", "init", "-q"],
                                ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                                 "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "fixture"],
                                ["cargo", "generate-lockfile", "--offline"]):
                    subprocess.run(command, cwd=root, env=env, capture_output=True, check=True, timeout=30)
                result = subprocess.run(
                    [sys.executable, "scripts/release.py", "build", "--target", release.host_target(),
                     "--output", str(root / "assets")], cwd=root, env=env,
                    capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                archive = Path(json.loads(result.stdout)["archive"])
                actual = root / "configured" / release.host_target() / "release/skillrail"
                with tarfile.open(archive, "r:gz") as packaged:
                    self.assertEqual(packaged.extractfile("skillrail").read(), actual.read_bytes())
                self.assertEqual(stale.read_text(), '#!/bin/sh\necho "skillrail 9.9.9"\n')

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
            binary.write_bytes(stub_binary(release.host_target()))
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
            binary.write_bytes(stub_binary("x86_64-unknown-linux-gnu", b" binary bytes"))
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
            binary.write_bytes(stub_binary(release.host_target()))
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
            binary.write_bytes(stub_binary(foreign))
            archive = release.package(binary, root / "a", foreign, "0.1.0", 123)
            with self.assertRaises(ValueError):
                release.verify(archive, run=True)

    def test_formula_requires_complete_actual_assets(self):
        with tempfile.TemporaryDirectory(prefix="s7-test-") as directory:
            root = Path(directory)
            binary = root / "skillrail"
            binary.write_bytes(stub_binary(release.host_target()))
            release.package(binary, root / "a", release.host_target(), "0.1.0", 123)
            with self.assertRaises(ValueError):
                release.formula(root / "a", "0.1.0")


HOST = release.host_target()
FOREIGN = [t for t in release.TARGETS if t != HOST]
SCRIPT = Path(__file__).with_name("release.py")
EVIDENCE = Path(__file__).with_name("native-release-evidence.py")


def run_script(script, *args, env=None):
    return subprocess.run([sys.executable, str(script), *args], capture_output=True, text=True,
                          timeout=60, env=env)


def assert_one_line_refusal(case, result, *needles):
    case.assertEqual(result.returncode, 1, result.stderr)
    lines = result.stderr.strip().splitlines()
    case.assertEqual(len(lines), 1, result.stderr)
    case.assertNotIn("Traceback", result.stderr)
    for needle in needles:
        case.assertIn(needle, lines[0])


def sidecar_for(archive):
    archive.with_suffix(archive.suffix + ".sha256").write_text(
        f"{release.digest(archive.read_bytes())}  {archive.name}\n")


def good_files(target=HOST, version="0.1.0"):
    files = {"skillrail": stub_binary(target), "LICENSE": b"license\n",
             "BUILD.json": (json.dumps({"version": version, "target": target, "source_date_epoch": 123},
                                       sort_keys=True) + "\n").encode()}
    return files


def with_sums(files):
    files = dict(files)
    files["SHA256SUMS"] = "".join(
        f"{release.digest(d)}  {n}\n" for n, d in sorted(files.items()) if n != "SHA256SUMS").encode()
    return files


def entries_for(files):
    return [{"name": n, "data": d} for n, d in sorted(files.items())]


def tar_bytes(entries, end=bytes(1024)):
    """Hand-built USTAR so a test can break exactly one record at a time."""
    out = bytearray()
    for entry in entries:
        data = entry["data"]
        info = tarfile.TarInfo(entry["name"])
        info.mode = entry.get("mode", 0o755 if entry["name"] == "skillrail" else 0o644)
        info.mtime = 123
        info.type = entry.get("type", tarfile.REGTYPE)
        info.size = len(data)
        if info.type == tarfile.SYMTYPE:
            info.linkname = "elsewhere"
        out += info.tobuf(tarfile.USTAR_FORMAT) + data + entry.get("pad", bytes(-len(data) % 512))
    return bytes(out) + end


def write_archive(directory, entries, name=None, end=bytes(1024), version="0.1.0"):
    archive = Path(directory) / (name or release.archive_name(version, HOST))
    archive.write_bytes(gzip.compress(tar_bytes(entries, end), mtime=0))
    sidecar_for(archive)
    return archive


class Guards(unittest.TestCase):
    """Each admission guard is the only reason its malformed archive is refused."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="s7-guard-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def refused(self, archive, text):
        with self.assertRaisesRegex(ValueError, text):
            release.verify(archive)

    def test_hand_built_baseline_is_accepted(self):
        archive = write_archive(self.root, entries_for(with_sums(good_files())))
        self.assertEqual(release.verify(archive)["target"], HOST)

    def test_archive_name_must_match_build_metadata(self):
        other = FOREIGN[0]
        archive = write_archive(self.root, entries_for(with_sums(good_files())),
                                name=release.archive_name("0.1.0", other))
        self.refused(archive, "archive metadata mismatch")

    def test_duplicate_member_refuses(self):
        # The four-header loop and the missing-member check also refuse any
        # duplicate, so this pins the outcome rather than a single guard.
        files = with_sums(good_files())
        entries = [e for e in entries_for(files) if e["name"] != "SHA256SUMS"]
        entries.insert(1, {"name": "LICENSE", "data": b"first license\n"})
        self.assertEqual([e["name"] for e in entries].count("LICENSE"), 2)
        self.refused(write_archive(self.root, entries), "archive members?")

    def test_nonzero_member_padding_refuses(self):
        entries = entries_for(with_sums(good_files()))
        entries[0]["pad"] = b"\x01" + bytes(-len(entries[0]["data"]) % 512 - 1)
        self.assertGreater(len(entries[0]["pad"]), 1)
        self.refused(write_archive(self.root, entries), "padding")

    def test_end_padding_must_be_all_zero(self):
        entries = entries_for(with_sums(good_files()))
        self.refused(write_archive(self.root, entries, end=bytes(1024) + b"\x01"), "end padding")
        self.refused(write_archive(self.root, entries, end=bytes(512)), "end padding")
        self.refused(write_archive(self.root, entries, end=bytes(512 + 10241)), "end padding")

    def test_member_size_limit(self):
        files = good_files()
        files["LICENSE"] = b"x" * 65537
        self.refused(write_archive(self.root, entries_for(with_sums(files))), "unexpected archive member")

    def test_member_type_must_be_regular(self):
        files = good_files()
        files["LICENSE"] = b""
        entries = entries_for(with_sums(files))
        for entry in entries:
            if entry["name"] == "LICENSE":
                entry["type"] = tarfile.SYMTYPE
        self.refused(write_archive(self.root, entries), "unexpected archive member")

    def test_compressed_budget(self):
        good = write_archive(self.root, entries_for(with_sums(good_files())))
        raw = good.read_bytes()
        # A 102 MiB gzip FCOMMENT keeps the stream valid and the checksum matching.
        big = self.root / "big" / good.name
        big.parent.mkdir()
        with big.open("wb") as out:
            out.write(raw[:3] + bytes([raw[3] | 0x10]) + raw[4:10])
            chunk = b"A" * (1024 * 1024)
            for _ in range(102):
                out.write(chunk)
            out.write(b"\0" + raw[10:])
        sidecar_for(big)
        self.refused(big, "budget")


class Executables(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="s7-exec-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / "skillrail"

    def package(self, binary_target, claimed, version="0.1.0", out="a"):
        self.binary.write_bytes(stub_binary(binary_target))
        return release.package(self.binary, self.root / out, claimed, version, 123)

    def test_every_foreign_target_label_is_refused(self):
        for claimed in FOREIGN:
            with self.subTest(claimed=claimed):
                archive = self.package(HOST, claimed, out=claimed)
                with self.assertRaisesRegex(ValueError, "executable"):
                    release.verify(archive)

    def test_each_target_accepts_its_own_executable(self):
        for target in release.TARGETS:
            with self.subTest(target=target):
                self.assertEqual(release.verify(self.package(target, target, out=target))["target"], target)

    def test_unrecognized_executable_is_refused(self):
        self.binary.write_bytes(b"#!/bin/sh\necho hi\n")
        archive = release.package(self.binary, self.root / "a", HOST, "0.1.0", 123)
        with self.assertRaisesRegex(ValueError, "executable"):
            release.verify(archive)

    def test_formula_refuses_mislabeled_assets_and_accepts_real_ones(self):
        assets = self.root / "four"
        for target in release.TARGETS:
            self.package(target, target, out="four")
        output = self.root / "full.rb"
        result = run_script(SCRIPT, "formula", "--assets", str(assets), "--output", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output.read_text().count("sha256 "), 4)
        mislabeled = FOREIGN[0]
        self.package(HOST, mislabeled, out="four")
        output.unlink()
        result = run_script(SCRIPT, "formula", "--assets", str(assets), "--output", str(output))
        assert_one_line_refusal(self, result, mislabeled, "executable")
        self.assertFalse(output.exists())

    def test_version_other_than_checkout_is_refused(self):
        archive = self.package(HOST, HOST, version="0.2.0")
        with self.assertRaisesRegex(ValueError, "version"):
            release.verify(archive)
        self.assertEqual(release.verify(archive, version="0.2.0")["version"], "0.2.0")


class Epoch(unittest.TestCase):
    BAD = ("abc", "-5", "99999999999999", "", " 5", "1.5", "٣")

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="s7-epoch-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_package_validates_epoch_before_writing(self):
        binary = self.root / "skillrail"
        binary.write_bytes(stub_binary(HOST))
        out = self.root / "out"
        out.mkdir()
        (out / "keep").write_text("x")
        for bad in (-5, 99999999999999, "abc", True):
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    release.package(binary, out, HOST, "0.1.0", bad)
                self.assertEqual([p.name for p in out.iterdir()], ["keep"])
        with self.assertRaises(ValueError):
            release.package(binary, self.root / "never-created", HOST, "0.1.0", -5)
        self.assertFalse((self.root / "never-created").exists())

    def test_successful_package_leaves_only_final_files(self):
        binary = self.root / "skillrail"
        binary.write_bytes(stub_binary(HOST))
        release.package(binary, self.root / "out", HOST, "0.1.0", 0)
        name = release.archive_name("0.1.0", HOST)
        self.assertEqual(sorted(p.name for p in (self.root / "out").iterdir()), [name, name + ".sha256"])

    def test_cli_build_refuses_bad_epoch_before_any_write(self):
        fake = self.root / "bin"
        fake.mkdir()
        (fake / "cargo").write_text("#!/bin/sh\nexit 99\n")
        (fake / "cargo").chmod(0o755)
        out = self.root / "out"
        out.mkdir()
        (out / "keep").write_text("x")
        for bad in self.BAD:
            with self.subTest(bad=bad):
                env = dict(os.environ, SOURCE_DATE_EPOCH=bad, PATH=str(fake) + os.pathsep + os.environ["PATH"])
                result = run_script(SCRIPT, "build", "--target", HOST, "--output", str(out), env=env)
                assert_one_line_refusal(self, result, "SOURCE_DATE_EPOCH")
                self.assertEqual([p.name for p in out.iterdir()], ["keep"])


class Refusals(unittest.TestCase):
    """Every refusal is one line naming the file and the problem."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="s7-refuse-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / "skillrail"
        self.binary.write_bytes(stub_binary(HOST))
        self.archive = release.package(self.binary, self.root / "a", HOST, "0.1.0", 123)

    def test_missing_sidecar(self):
        self.archive.with_suffix(".gz.sha256").unlink()
        assert_one_line_refusal(self, run_script(SCRIPT, "verify", str(self.archive)),
                                self.archive.name, "sha256", "missing")

    def test_truncated_archive(self):
        self.archive.write_bytes(self.archive.read_bytes()[:-40])
        sidecar_for(self.archive)
        assert_one_line_refusal(self, run_script(SCRIPT, "verify", str(self.archive)),
                                self.archive.name, "gzip")

    def test_random_bytes(self):
        self.archive.write_bytes(os.urandom(4096))
        sidecar_for(self.archive)
        assert_one_line_refusal(self, run_script(SCRIPT, "verify", str(self.archive)),
                                self.archive.name, "gzip")

    def test_wrong_version(self):
        other = release.package(self.binary, self.root / "b", HOST, "0.2.0", 123)
        assert_one_line_refusal(self, run_script(SCRIPT, "verify", str(other)),
                                other.name, "0.2.0", "0.1.0")

    def test_missing_archive_and_wrong_binary_version(self):
        assert_one_line_refusal(self, run_script(SCRIPT, "verify", str(self.root / "nope.tar.gz")), "nope.tar.gz")
        shell = self.root / "sh-binary"
        shell.write_text('#!/bin/sh\necho "skillrail 9.9.9"\n')
        shell.chmod(0o755)
        with self.assertRaisesRegex(AssertionError, "9.9.9"):
            release.smoke(shell, "0.1.0")

    def test_debug_flag_restores_traceback(self):
        result = run_script(SCRIPT, "--debug", "verify", str(self.root / "nope.tar.gz"))
        self.assertEqual(result.returncode, 1)
        self.assertIn("Traceback", result.stderr)

    def test_run_refuses_missing_tmpdir(self):
        env = dict(os.environ, TMPDIR=str(self.root / "does-not-exist"))
        result = run_script(SCRIPT, "verify", str(self.archive), "--run", env=env)
        assert_one_line_refusal(self, result, "TMPDIR", "does-not-exist")

    def test_native_evidence_refusals(self):
        missing = self.root / "no-such-skillrail"
        result = run_script(EVIDENCE, "https", "--binary", str(missing))
        assert_one_line_refusal(self, result, "no-such-skillrail")
        fake = self.root / "claude"
        fake.write_text('#!/bin/sh\necho "1.0.0 (Claude Code)"\n')
        fake.chmod(0o755)
        result = run_script(EVIDENCE, "claude", "--binary", str(fake), "--claude-binary", str(fake))
        assert_one_line_refusal(self, result, str(fake), "1.0.0")

if __name__ == "__main__":
    unittest.main()
