#!/usr/bin/env python3
"""Executable CLI and PTY checks; all application paths are disposable fixtures."""
import copy
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import sqlite3
import struct
import subprocess
import tempfile
import termios
import time


def command(binary, root):
    return [str(binary), "--home", str(root / "home"), "--project",
            str(root / "project"), "--state-dir", str(root / "state")]


def cli_smoke(binary, root):
    outputs = []
    for verb in ("scan", "list"):
        result = subprocess.run(command(binary, root) + [verb, "--json"],
                                capture_output=True, check=True)
        assert not result.stderr, result.stderr
        inventory = json.loads(result.stdout)
        assert inventory["schema_version"] == 1
        assert inventory["items"] == inventory["occurrences"] == []
        assert inventory["discovery"] == {"status": "not_implemented", "adapters": []}
        assert inventory["diagnostics"][0]["code"] == "discovery_not_implemented"
        assert inventory["context"] == {"home": str(root / "home"),
                                        "project": str(root / "project"),
                                        "state_dir": str(root / "state")}
        assert inventory["scanned_at_unix_ms"] > 0
        normalized = copy.deepcopy(inventory)
        del normalized["scanned_at_unix_ms"]
        outputs.append(normalized)
        (root / "state/index.sqlite3").unlink()
    assert outputs[0] == outputs[1]
    print("CLI smoke: isolated scan/list JSON and cache rebuild passed")


def tui_smoke(binary, root, fail_refresh=False):
    master, slave = pty.openpty()
    process = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 16, 100, 0, 0))
        original = termios.tcgetattr(slave)
        process = subprocess.Popen(command(binary, root) + ["tui"], stdin=slave,
                                   stdout=slave, stderr=slave, start_new_session=True)
        output = bytearray()
        deadline = time.monotonic() + 10
        while b"Discovery is not implemented in M0." not in b" ".join(
                re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b" ", output).split()):
            assert time.monotonic() < deadline, f"TUI failed to render before timeout: {output!r}"
            assert process.poll() is None, f"TUI exited before rendering: {output!r}"
            if select.select([master], [], [], 0.1)[0]:
                output.extend(os.read(master, 65536))
        if fail_refresh:
            with sqlite3.connect(root / "state/index.sqlite3") as connection:
                connection.execute("PRAGMA user_version = 999")
            os.write(master, b"r")
        else:
            os.write(master, b"q")
        assert process.wait(timeout=10) == (1 if fail_refresh else 0)
        while select.select([master], [], [], 0)[0]:
            output.extend(os.read(master, 65536))
        assert termios.tcgetattr(slave) == original, "terminal attributes were not restored"
        assert b"\x1b[?1049h" in output and b"\x1b[?1049l" in output, "alternate screen not restored"
        if fail_refresh:
            assert b"index_version_unsupported" in output
        print(f"PTY smoke: {'refresh error' if fail_refresh else 'quit'} restored terminal")
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]))
    target = Path(metadata["target_directory"])
    binary = target / "debug/skillrail"
    for scenario in ("cli", "quit", "error"):
        with tempfile.TemporaryDirectory(prefix="m0-smoke-", dir=target) as directory:
            root = Path(directory).resolve()
            (root / "home").mkdir()
            (root / "project").mkdir()
            if scenario == "cli":
                cli_smoke(binary, root)
            else:
                tui_smoke(binary, root, fail_refresh=scenario == "error")


if __name__ == "__main__":
    main()
