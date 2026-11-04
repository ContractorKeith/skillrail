#!/usr/bin/env python3
"""Stateful package-manager failures, with no real Homebrew mutations."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
import release
from release_fixtures import stub_binary

ROOT = Path(__file__).resolve().parent.parent


class Failures(unittest.TestCase):
    def run_case(self, scenario, extra_env=None):
        temporary = tempfile.TemporaryDirectory(prefix="b7-failure-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        tools = root / "tools"
        tools.mkdir()
        stub = root / "stub"
        stub.write_bytes(stub_binary(release.host_target(), b"inert"))
        release.package(stub, root / "assets", release.host_target(), "0.1.0", 123)
        fake = tools / "brew"
        fake.write_text('''#!/usr/bin/env python3
import json, os, subprocess, sys, shutil
from pathlib import Path
r = Path(os.environ["BREW_TEST_ROOT"])
s = os.environ["BREW_TEST_SCENARIO"]
a = sys.argv[1:]
with (r / "calls").open("a") as f: f.write(json.dumps(a) + "\\n")
tap = r / "tap"
installed = r / "installed"
if a == ["--version"]: pass
elif a == ["list", "--versions", "skillrail"]: sys.exit(1)
elif a == ["list", "--formula", "--versions"]:
    if s == "install-list-fail": sys.exit(7)
    if installed.exists() or s == "existing": print("skillrail 0.1.0")
elif a == ["tap"]:
    if s == "tap-list-fail": sys.exit(7)
    if tap.exists() or s == "tap-existing": print("skillrail-check/candidate")
elif a[0] == "--repo": print(tap)
elif a[0] == "tap":
    subprocess.run(["git", "init", "--template=", "-q", str(tap)], check=True)
    origin = "unrelated" if s == "foreign-partial-tap" else a[2]
    subprocess.run(["git", "-C", str(tap), "remote", "add", "origin", origin], check=True)
    if s in ("partial-tap", "foreign-partial-tap"): sys.exit(7)
elif a[0] == "install": installed.touch()
elif a[0] == "test":
    if s == "hang":
        import time
        time.sleep(15)
        sys.exit(0)
    sys.exit(7)
elif a[0] == "uninstall":
    if s == "uninstall-fail": sys.exit(8)
    installed.unlink(missing_ok=True)
    if s == "uninstall-failed-but-absent": sys.exit(8)
elif a[0] == "untap":
    if s == "untap-fail": sys.exit(9)
    shutil.rmtree(tap)
else: sys.exit(20)
''')
        fake.chmod(0o755)
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                   TMPDIR=str(root), BREW_TEST_ROOT=str(root), BREW_TEST_SCENARIO=scenario,
                   GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1", **(extra_env or {}))
        result = subprocess.run(["bash", "scripts/validate-brew.sh", str(root / "assets")],
                                cwd=ROOT, env=env, capture_output=True, timeout=60)
        calls = [json.loads(line) for line in (root / "calls").read_text().splitlines()]
        return root, result, calls

    def test_failures_clean_independently_safe_stages(self):
        for scenario in ("test-fail", "partial-tap", "uninstall-failed-but-absent"):
            with self.subTest(scenario=scenario):
                root, result, calls = self.run_case(scenario)
                self.assertEqual(result.returncode, 7, result.stderr.decode())
                self.assertFalse((root / "tap").exists())
                self.assertFalse((root / "installed").exists())
                self.assertEqual(list(root.glob("b7.*")), [])
                self.assertIn(["untap", "skillrail-check/candidate"], calls)

    def test_discovery_failure_or_existing_state_never_mutates(self):
        for scenario in ("tap-list-fail", "install-list-fail", "existing", "tap-existing"):
            with self.subTest(scenario=scenario):
                _, result, calls = self.run_case(scenario)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(any(c[0] in ("install", "uninstall", "untap") or
                                     (c[0] == "tap" and len(c) > 1) for c in calls), calls)

    def test_cleanup_errors_preserve_recovery_and_original_failure(self):
        for scenario in ("uninstall-fail", "untap-fail", "foreign-partial-tap"):
            with self.subTest(scenario=scenario):
                root, result, calls = self.run_case(scenario)
                self.assertEqual(result.returncode, 7, result.stderr.decode())
                self.assertTrue((root / "tap").exists())
                self.assertEqual(len(list(root.glob("b7.*"))), 1)
                self.assertIn("retained", result.stderr.decode())
                if scenario != "untap-fail":
                    self.assertNotIn(["untap", "skillrail-check/candidate"], calls)

    def test_hung_brew_command_times_out_and_cleans_up(self):
        started = time.monotonic()
        root, result, calls = self.run_case("hang", {"SKILLRAIL_BREW_TIMEOUT": "1"})
        self.assertLess(time.monotonic() - started, 12, "validator waited for the hung command")
        self.assertEqual(result.returncode, 124, result.stderr.decode())
        self.assertFalse((root / "tap").exists())
        self.assertFalse((root / "installed").exists())
        self.assertIn(["untap", "skillrail-check/candidate"], calls)


if __name__ == "__main__":
    unittest.main()
