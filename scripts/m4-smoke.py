#!/usr/bin/env python3
"""M4 real PTY preview/cancel/apply/history/undo. No agent sessions or user paths."""
import fcntl
import argparse
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


def main(read_size=65536):
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

            def frame_end():
                # Pinned Ratatui/Crossterm emits Hide after each complete draw.
                marker = b"\x1b[?25l"
                at = output.rfind(marker)
                return at + len(marker) if at >= 0 else 0

            def wait_for(predicate, description, after=0):
                deadline = time.monotonic() + 10
                # Never match old text remaining beneath a partially drawn screen.
                while frame_end() <= after or not predicate(screen_text(output[:frame_end()], 40, 180)):
                    assert time.monotonic() < deadline, f"{description}: {screen_text(output, 40, 180)!r}"
                    assert process.poll() is None, f"exited before {description}"
                    if select.select([master], [], [], 0.1)[0]:
                        output.extend(os.read(master, read_size))

            def send(keys, needle):
                previous_frame = frame_end()
                os.write(master, keys)
                wait_for(lambda text: needle in text, needle, after=previous_frame)

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
            # A PTY read may end after the header but before the preview body.
            wait_for(lambda text: '"action": "undo"' in text, "undo preview body")
            send(b"\x1b", "Skills/MCP: 1")
            assert config.read_bytes() != original_bytes
            send(b"h", "Operation history")
            send(b"u", "Enter: apply")
            send(b"\r", "undone")
            assert config.read_bytes() == original_bytes
            send(b"\x1b", "Skills/MCP: 1")
            send(b"h", "Operation history")
            wait_for(lambda text: "Operation history" in text and "undone" in text, "undone history entry")
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
    parser = argparse.ArgumentParser()
    parser.add_argument("--read-size", type=int, choices=(64, 65536), default=65536)
    main(parser.parse_args().read_size)
