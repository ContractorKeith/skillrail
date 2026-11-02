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
import signal
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
        assert len(inventory["items"]) == 3
        assert len(inventory["occurrences"]) == 5
        assert any(i["name_collision"] for i in inventory["items"])
        assert any(i["drift"] == "drifted" for i in inventory["items"])
        assert len({o["canonical_path"] for o in inventory["occurrences"]}) == 4
        assert inventory["discovery"] == {"status": "inventory_partial", "adapters": ["claude_standalone_v1", "codex_standalone_v1", "claude_mcp_plugins_v1", "codex_mcp_plugins_v1"]}
        assert inventory["diagnostics"][0]["code"] == "coverage_partial"
        assert inventory["context"] == {"home": str(root / "home"),
                                        "project": str(root / "project"),
                                        "state_dir": str(root / "state")}
        assert inventory["scanned_at_unix_ms"] > 0
        normalized = copy.deepcopy(inventory)
        del normalized["scanned_at_unix_ms"]
        normalized.pop("page", None)
        outputs.append(normalized)
        (root / "state/index.sqlite3").unlink()
    assert outputs[0] == outputs[1]
    drift = json.loads(subprocess.check_output(command(binary, root) + ["drift", "--json"]))
    assert len(drift["items"]) == 1 and drift["items"][0]["drift"] == "drifted"
    print("CLI smoke: isolated scan/list JSON and cache rebuild passed")


def screen_text(data, rows=16, columns=100):
    """Replay cursor-positioned terminal deltas for assertions on visible text."""
    screen = [[" "] * columns for _ in range(rows)]
    row = column = 0
    for match in re.finditer(r"\x1b\[[0-?]*[ -/]*[@-~]|[^\x1b]", data.decode("utf-8", errors="replace")):
        token = match.group()
        if token.startswith("\x1b["):
            if token[-1] in "Hf":
                position = token[2:-1].split(";")
                row = int(position[0] or 1) - 1
                column = int(position[1] or 1) - 1 if len(position) > 1 else 0
            elif token == "\x1b[2J":
                screen = [[" "] * columns for _ in range(rows)]
        elif token == "\r":
            column = 0
        elif token == "\n":
            row += 1
        elif token.isprintable():
            if 0 <= row < rows and 0 <= column < columns:
                screen[row][column] = token
            column += 1
    return "\n".join("".join(line) for line in screen)


def tui_smoke(binary, root, fail_refresh=False, exit_backpressure=False):
    master, slave = pty.openpty()
    process = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 16, 100, 0, 0))
        original = termios.tcgetattr(slave)
        process = subprocess.Popen(command(binary, root) + ["tui"], stdin=slave,
                                   stdout=slave, stderr=slave, start_new_session=True)
        output = bytearray()
        deadline = time.monotonic() + 10
        while b"Skills/MCP: 3" not in b" ".join(
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
            # Exercise a real successful refresh, selection and resize before quitting.
            added = root / "project/.agents/skills/refreshed"
            added.mkdir()
            (added / "SKILL.md").write_text("---\nname: refreshed\ndescription: Added after startup\n---\n")
            os.write(master, b"r")
            refreshed = False
            while not refreshed:
                assert time.monotonic() < deadline, "TUI refresh did not publish the new fixture"
                with sqlite3.connect(root / "state/index.sqlite3") as connection:
                    row = connection.execute("SELECT inventory_json FROM scan").fetchone()
                refreshed = len(json.loads(row[0])["items"]) == 4
                if select.select([master], [], [], 0.1)[0]:
                    output.extend(os.read(master, 65536))
            # Search is typed through the real PTY, then Enter opens matching details.
            os.write(master, b"/refreshed\r")
            while "Search: refreshed |" not in screen_text(output) or "Added after startup" not in screen_text(output):
                assert time.monotonic() < deadline, "TUI search did not select matching details"
                if select.select([master], [], [], 0.1)[0]:
                    output.extend(os.read(master, 65536))
            os.write(master, b"aa")
            while "agent: codex" not in screen_text(output):
                assert time.monotonic() < deadline, "TUI agent filter did not change"
                if select.select([master], [], [], 0.1)[0]:
                    output.extend(os.read(master, 65536))
            os.write(master, b"\x1b[B\x1b[6~")
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 30, 0, 0))
            os.kill(process.pid, signal.SIGWINCH)
            while not screen_text(output, 8, 30).splitlines()[7].startswith("└"):
                assert time.monotonic() < deadline, "TUI did not redraw the resized terminal"
                if select.select([master], [], [], 0.1)[0]:
                    output.extend(os.read(master, 65536))
            if exit_backpressure:
                # Queue enough redraws to fill a PTY unless the parent drains it.
                os.write(master, b"\x1b[6~\x1b[5~" * 100)
            os.write(master, b"q")
        # Keep draining while waiting: redraw/restoration writes can fill a
        # PTY, especially on macOS, and otherwise prevent the child from exiting.
        exit_deadline = time.monotonic() + 10
        while process.poll() is None:
            assert time.monotonic() < exit_deadline, "TUI did not exit while output was drained"
            if select.select([master], [], [], 0.1)[0]:
                output.extend(os.read(master, 65536))
        assert process.returncode == (1 if fail_refresh else 0)
        while select.select([master], [], [], 0)[0]:
            output.extend(os.read(master, 65536))
        assert termios.tcgetattr(slave) == original, "terminal attributes were not restored"
        assert b"\x1b[?1049h" in output and b"\x1b[?1049l" in output, "alternate screen not restored"
        if fail_refresh:
            assert b"index_version_unsupported" in output
        print(f"PTY smoke: {'refresh error' if fail_refresh else 'backpressured quit' if exit_backpressure else 'quit'} restored terminal")
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def seed(root):
    (root / "project/.git").mkdir()
    (root / "home/.git").mkdir()
    for relative in ("home/.claude/skills/copy", "project/.agents/skills/copy",
                     "home/.agents/skills/check", "project/.claude/skills/check"):
        folder = root / relative
        folder.mkdir(parents=True)
        name = folder.name
        (folder / "SKILL.md").write_text(f"---\nname: {name}\ndescription: Fixture skill\n---\n")
        (folder / "script.sh").write_text(relative)
    for relative in ("home/.claude", "project/.agents"):
        git = root / relative / ".git"
        git.mkdir()
        (git / "config").write_text('[remote "origin"]\nurl = https://github.com/example/skills.git\n')
    (root / "home/.claude/skills/alias").symlink_to(root / "home/.claude/skills/copy", target_is_directory=True)


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]))
    target = Path(metadata["target_directory"])
    binary = target / "debug/skillrail"
    for scenario in ("cli", "quit", "error", "backpressure"):
        with tempfile.TemporaryDirectory(prefix="m1-smoke-", dir=target) as directory:
            root = Path(directory).resolve()
            (root / "home").mkdir()
            (root / "project").mkdir()
            seed(root)
            if scenario == "cli":
                cli_smoke(binary, root)
            else:
                tui_smoke(binary, root, fail_refresh=scenario == "error",
                          exit_backpressure=scenario == "backpressure")


if __name__ == "__main__":
    main()
