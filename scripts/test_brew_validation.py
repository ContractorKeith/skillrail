#!/usr/bin/env python3
"""Run the validator through a fake brew boundary that models autoremove.

Uses the real built Skillrail for installed-binary smokes; no real tap.
The fake package manager models cleanup, not artifact extraction.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import release

ROOT = Path(__file__).resolve().parent.parent


class BrewCleanup(unittest.TestCase):
    def test_uninstall_retains_unrelated_packages(self):
        with tempfile.TemporaryDirectory(prefix="b7-test-") as directory:
            root = Path(directory)
            tools = root / "tools"
            tools.mkdir()
            prefix = root / "installed"
            (prefix / "bin").mkdir(parents=True)
            binary = ROOT / "target/debug/skillrail"
            (prefix / "bin/skillrail").symlink_to(binary)
            assets = root / "assets"
            stub = root / "archive-fixture"
            stub.write_bytes(b"inert archive fixture")
            release.package(stub, assets, release.host_target(), "0.1.0", 123)
            unrelated = root / "unrelated-package"
            unrelated.write_text("pre-existing")
            fake = tools / "brew"
            fake.write_text('''#!/usr/bin/env python3
import os
from pathlib import Path
import sys
args = sys.argv[1:]
if args == ["list", "--versions", "skillrail"]:
    sys.exit(1)
if args == ["tap"]:
    sys.exit(0)
if args[0] == "--prefix":
    print(os.environ["BREW_TEST_PREFIX"])
if args[0] == "uninstall" and os.environ.get("HOMEBREW_NO_AUTOREMOVE") != "1":
    Path(os.environ["BREW_TEST_UNRELATED"]).unlink()
''')
            fake.chmod(0o755)
            env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                       TMPDIR=str(root), BREW_TEST_PREFIX=str(prefix),
                       BREW_TEST_UNRELATED=str(unrelated))
            # Do not inherit a setting that could mask a missing script guard.
            env.pop("HOMEBREW_NO_AUTOREMOVE", None)
            env.pop("HOMEBREW_NO_INSTALL_CLEANUP", None)
            result = subprocess.run(["bash", "scripts/validate-brew.sh", str(assets)],
                                    env=env, cwd=ROOT, capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertTrue(unrelated.exists(), "validator cleanup autoremoved unrelated package")


if __name__ == "__main__":
    unittest.main()
