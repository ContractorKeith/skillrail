#!/usr/bin/env python3
"""M4 real PTY preview/cancel/apply/history/undo. No agent sessions or user paths."""
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
    target = Path(metadata["target_directory"])
    binary = target / "debug/skillrail"
    # Build caches may have writable ancestors; mutation fixtures need trusted paths.
    with tempfile.TemporaryDirectory(prefix="m4-") as directory:
        root = Path(directory).resolve()
        (root / "home/.codex").mkdir(parents=True)
        (root / "project/.git").mkdir(parents=True)
        config = root / "home/.codex/config.toml"
        original_bytes = b"# PTY preserved\n[mcp_servers.fixture]\ncommand = 'never-execute'\nargs = ['PRIVATE_PTY_CANARY']\n"
        config.write_bytes(original_bytes)
        master, slave = pty.openpty()
        process = None
        output = bytearray()
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
            original_terminal = termios.tcgetattr(slave)
            process = subprocess.Popen(command(binary, root) + ["tui"], stdin=slave, stdout=slave, stderr=slave, start_new_session=True)

            def wait_for(predicate, description):
                deadline = time.monotonic() + 10
                while not predicate(screen_text(output, 40, 180)):
                    assert time.monotonic() < deadline, f"{description}: {screen_text(output, 40, 180)!r}"
                    assert process.poll() is None, f"exited before {description}"
                    if select.select([master], [], [], 0.1)[0]:
                        output.extend(os.read(master, 65536))

            def send(keys, needle):
                os.write(master, keys)
                wait_for(lambda text: needle in text, needle)

            wait_for(lambda text: "Skills/MCP: 1" in text, "initial inventory")
            send(b"b", "Select exact occurrence")
            send(b"\r", "Enter: apply")
            assert config.read_bytes() == original_bytes
            assert not (root / "state/journal").exists()
            send(b"\x1b", "Skills/MCP: 1")
            assert config.read_bytes() == original_bytes
            send(b"b", "Select exact occurrence")
            send(b"\r", "Enter: apply")
            send(b"\r", "applied")
            assert b"enabled = false" in config.read_bytes()
            send(b"\x1b", "Skills/MCP: 1")
            send(b"h", "Operation history")
            send(b"u", "Enter: apply")
            assert '"action": "undo"' in screen_text(output, 40, 180)
            send(b"\x1b", "Skills/MCP: 1")
            assert config.read_bytes() != original_bytes
            send(b"h", "Operation history")
            send(b"u", "Enter: apply")
            send(b"\r", "undone")
            assert config.read_bytes() == original_bytes
            send(b"\x1b", "Skills/MCP: 1")
            send(b"h", "Operation history")
            assert "undone" in screen_text(output, 40, 180)
            send(b"\x1b", "Skills/MCP: 1")
            os.write(master, b"q")
            deadline = time.monotonic() + 10
            while process.poll() is None:
                assert time.monotonic() < deadline, "quit stalled while output drained"
                if select.select([master], [], [], 0.1)[0]:
                    output.extend(os.read(master, 65536))
            assert process.returncode == 0
            restored = termios.tcgetattr(slave)
            assert terminal_attributes_equal(original_terminal, restored), f"before={original_terminal!r}; after={restored!r}"
            assert b"\x1b[?1049h" in output and b"\x1b[?1049l" in output
            assert b"PRIVATE_PTY_CANARY" not in output
            print("M4 PTY: select/preview/cancel/apply/history/undo/cancel/restoration and credential canary passed")
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main()
