#!/usr/bin/env python3
"""M6 public CLI local/Git lifecycle and real PTY preview/cancel/apply/history/undo."""
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time
import sys
sys.dont_write_bytecode = True
from smoke import command, screen_text, terminal_attributes_equal


def main():
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]))
    binary = Path(metadata["target_directory"]) / "debug/skillrail"
    with tempfile.TemporaryDirectory(prefix="m6-") as directory:
        root = Path(directory).resolve()
        (root / "home").mkdir()
        (root / "project/.git").mkdir(parents=True)
        source = root / "source"
        source.mkdir()
        (source / "SKILL.md").write_text("---\nname: demo\ndescription: fixture\n---\nM6_PRIVATE_BODY_CANARY\n")
        (root / "home/.codex").mkdir()
        config = root / "home/.codex/config.toml"
        config.write_text('unrelated = "M6_CONFIG_SECRET_CANARY"\n')

        def cli(*args, code=0):
            out = subprocess.run(command(binary, root) + list(args) + ["--json"], capture_output=True, check=False)
            assert out.returncode == code, (args, out.returncode, out.stderr)
            assert b"M6_PRIVATE_BODY_CANARY" not in out.stdout + out.stderr
            assert b"M6_CONFIG_SECRET_CANARY" not in out.stdout + out.stderr
            return json.loads(out.stdout) if code == 0 else None

        installed = cli("install", str(source), "--agent", "codex", "--scope", "user", "--apply")
        owned = installed["install"]["owned_id"]
        link = root / "home/.agents/skills/demo"
        original = link.resolve()
        assert cli("update", owned, "--apply")["status"] == "unchanged"
        (source / "asset").write_bytes(bytes([0, 255, 2]))
        preview = cli("update", owned)
        assert [d["path"] for d in preview["content_diff"]] == ["asset"]
        (source / "asset").write_bytes(bytes([0, 255, 2, 3]))
        cli("update", owned, "--apply", "--expect", preview["token"], code=4)
        assert link.resolve() == original

        master, slave = pty.openpty()
        process = None
        output = bytearray()
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 45, 190, 0, 0))
            original_terminal = termios.tcgetattr(slave)
            process = subprocess.Popen(command(binary, root) + ["tui"], stdin=slave, stdout=slave, stderr=slave, start_new_session=True)

            def wait_for(predicate, description):
                deadline = time.monotonic() + 10
                while not predicate(screen_text(output, 45, 190)):
                    assert time.monotonic() < deadline, f"{description}: {screen_text(output, 45, 190)!r}"
                    assert process.poll() is None, f"exited before {description}"
                    if select.select([master], [], [], 0.1)[0]:
                        output.extend(os.read(master, 65536))

            def send(keys, needle):
                os.write(master, keys)
                wait_for(lambda text: needle in text, needle)

            wait_for(lambda text: "Skills/MCP: 1" in text, "inventory")
            send(b"u", '"action": "update"')
            assert link.resolve() == original
            # Scroll the retained diff so the actual file summary is visible in the PTY.
            os.write(master, b"\x1b[6~" * 8)
            wait_for(lambda text: '"content_diff"' in text or '"sha256"' in text or '"size"' in text, "content diff")
            send(b"\x1b", "Skills/MCP: 1")
            assert link.resolve() == original
            send(b"u", '"action": "update"')
            send(b"\r", "applied")
            updated = link.resolve()
            assert updated != original and (updated / "asset").read_bytes() == bytes([0, 255, 2, 3])
            send(b"\x1b", "Skills/MCP: 1")
            send(b"D", '"action": "uninstall"')
            send(b"\x1b", "Skills/MCP: 1")
            assert link.exists()
            send(b"D", '"action": "uninstall"')
            send(b"\r", "applied")
            assert not link.exists() and not updated.exists()
            send(b"\x1b", "Skills/MCP: 0")
            send(b"h", "Operation history")
            operations = cli("operations")["operations"]
            index = next(i for i, op in enumerate(operations) if op["action"] == "uninstall")
            if index:
                os.write(master, b"\x1b[B" * index)
            send(b"u", '"action": "undo"')
            send(b"\x1b", "Skills/MCP: 0")
            assert not link.exists()
            send(b"h", "Operation history")
            if index:
                os.write(master, b"\x1b[B" * index)
            send(b"u", '"action": "undo"')
            send(b"\r", "undone")
            assert link.resolve() == updated
            send(b"\x1b", "Skills/MCP: 1")
            send(b"h", "Operation history")
            update_index = next(i for i, op in enumerate(operations) if op["action"] == "update")
            if update_index:
                os.write(master, b"\x1b[B" * update_index)
            send(b"u", '"action": "undo"')
            send(b"\r", "undone")
            assert link.resolve() == original
            send(b"\x1b", "Skills/MCP: 1")
            os.write(master, b"q")
            deadline = time.monotonic() + 10
            while process.poll() is None:
                assert time.monotonic() < deadline, "quit stalled"
                if select.select([master], [], [], 0.1)[0]:
                    output.extend(os.read(master, 65536))
            assert process.returncode == 0
            assert terminal_attributes_equal(original_terminal, termios.tcgetattr(slave))
            assert b"\x1b[?1049h" in output and b"\x1b[?1049l" in output
            assert b"M6_PRIVATE_BODY_CANARY" not in output and b"M6_CONFIG_SECRET_CANARY" not in output
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)

        update_op = next(op["operation_id"] for op in cli("operations")["operations"] if op["action"] == "update")
        cli("undo", update_op, "--apply")
        assert link.resolve() == original
        (original / "unknown").write_text("preserved")
        cli("uninstall", owned, "--apply", code=4)
        assert (original / "unknown").read_text() == "preserved"
        (original / "unknown").unlink()
        cli("undo", installed["operation_id"], "--apply")
        assert config.read_text() == 'unrelated = "M6_CONFIG_SECRET_CANARY"\n'
        assert b"M6_PRIVATE_BODY_CANARY" not in (root / "state/index.sqlite3").read_bytes()

        env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null")
        def git(*args):
            return subprocess.check_output(["git", "-C", str(source), "-c", "core.hooksPath=/dev/null", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test", *args], env=env, stderr=subprocess.PIPE).decode().strip()
        git("init", "--template=")
        git("add", ".")
        git("commit", "-m", "first")
        installed = cli("install", str(source), "--git", "--ref", "HEAD", "--agent", "codex", "--scope", "user", "--apply")
        owned = installed["install"]["owned_id"]
        first = link.resolve()
        (source / "asset").write_bytes(bytes([9, 0, 255]))
        git("add", ".")
        git("commit", "-m", "second")
        rev = git("rev-parse", "HEAD")
        preview = cli("update", owned)
        assert preview["install"]["provenance"]["revision"] == rev
        updated = cli("update", owned, "--apply", "--expect", preview["token"])
        current = updated["install"]["owned_id"]
        assert cli("update", current, "--apply")["status"] == "unchanged"
        removed = cli("uninstall", current, "--apply")
        assert not link.exists()
        cli("undo", removed["operation_id"], "--apply")
        cli("undo", updated["operation_id"], "--apply")
        assert link.resolve() == first
        cli("undo", installed["operation_id"], "--apply")
        print("M6 public CLI local/Git update/noop/uninstall/undo/refusals and real PTY diff/cancel/apply/history/undo/restoration/canaries passed")


if __name__ == "__main__":
    main()
