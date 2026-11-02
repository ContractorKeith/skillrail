#!/usr/bin/env python3
"""Raw stdio boundary probes complement the official Rust SDK client test."""
import argparse
import json
import os
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile
import time


class Client:
    def __init__(self, binary, root):
        self.process = subprocess.Popen(
            [str(binary), "--home", str(root / "home"), "--project", str(root / "project"),
             "--state-dir", str(root / "state"), "mcp"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.pending = bytearray()

    def send(self, message):
        self.raw(json.dumps(message).encode() + b"\n")

    def raw(self, data):
        self.process.stdin.write(data)
        self.process.stdin.flush()

    def read(self):
        deadline = time.monotonic() + 8
        while b"\n" not in self.pending:
            assert time.monotonic() < deadline, "MCP response timed out"
            assert self.process.poll() is None, "MCP exited before responding"
            if select.select([self.process.stdout], [], [], 0.1)[0]:
                data = os.read(self.process.stdout.fileno(), 65536)
                assert data, "MCP stdout closed"
                self.pending.extend(data)
            assert len(self.pending) <= 2 * 1024 * 1024, "unbounded protocol output"
        line, _, self.pending = self.pending.partition(b"\n")
        return json.loads(line)

    def request(self, ident, method, params=None):
        message = {"jsonrpc": "2.0", "id": ident, "method": method}
        if params is not None:
            message["params"] = params
        self.send(message)
        response = self.read()
        assert response.get("id") == ident, response
        return response

    def close(self):
        self.process.stdin.close()
        assert self.process.wait(timeout=5) == 0
        assert not self.pending
        assert not self.process.stdout.read(), "unexpected stdout after disconnect"
        assert not self.process.stderr.read(), "protocol/parser data reached stderr"

    def cleanup(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait()


def scenario(binary, root, version):
    client = Client(binary, root)
    try:
        initialized = client.request(1, "initialize", {
            "protocolVersion": version, "capabilities": {},
            "clientInfo": {"name": "skillrail-boundary-probe", "version": "1"}})
        assert initialized["result"]["protocolVersion"] in (
            "2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"), initialized
        if version != "1999-01-01":
            assert initialized["result"]["protocolVersion"] == version
        assert set(initialized["result"]["capabilities"]) == {"tools"}
        client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        tools = client.request(2, "tools/list")["result"]["tools"]
        assert len(tools) == 5
        assert all(t["annotations"]["readOnlyHint"] for t in tools)
        # SDK transport discards syntax errors and oversized lines, then resumes.
        client.raw(b'{BROKEN_CREDENTIAL_CANARY\n')
        client.raw(b'X' * (64 * 1024 + 1) + b'\n')
        assert client.request(3, "ping")["result"] == {}
        # Structurally invalid JSON-RPC gets a sanitized Invalid Request.
        client.raw(b'{"jsonrpc":"2.0","id":{},"method":"ping","params":{"secret":"DO_NOT_ECHO"}}\n')
        malformed = client.read()
        assert malformed["error"]["code"] == -32600, malformed
        assert "DO_NOT_ECHO" not in json.dumps(malformed)
        # Pipeline responses and malformed shapes to exercise the service's
        # send/receive cancellation boundary through real OS pipes.
        batch = []
        ping_ids = set()
        for index in range(16):
            for ident in (1000 + 2 * index, 1001 + 2 * index):
                ping_ids.add(ident)
                batch.append({"jsonrpc":"2.0", "id":ident, "method":"ping"})
                if ident % 2 == 0:
                    batch.append({"jsonrpc":"2.0", "id":{}, "method":"ping",
                                  "params":{"secret":"PIPELINE_CANARY"}})
        client.raw(b"".join(json.dumps(message).encode() + b"\n" for message in batch))
        replies = [client.read() for _ in batch]
        errors = [reply for reply in replies if "error" in reply]
        assert len(errors) == 16 and all(reply["error"]["code"] == -32600 and reply.get("id") is None for reply in errors)
        assert {reply["id"] for reply in replies if "result" in reply} == ping_ids
        assert "PIPELINE_CANARY" not in json.dumps(replies)
        for ident, method, params in [
            (4, "tools/call", {"name": "write_config", "arguments": {}}),
            (5, "tools/call", {"name": "get_item", "arguments": {}}),
            (6, "tools/call", {"name": "search_inventory", "arguments": {"query": " "}}),
            (7, "tools/call", {"name": "list_inventory", "arguments": {"drift_only": True}}),
            (8, "tools/list", {"cursor": "invalid"}),
        ]:
            response = client.request(ident, method, params)
            assert response["error"]["code"] == -32602, response
        # No raw resources, skill bodies or mutation capability.
        # The pinned SDK rejects malformed outer params before tool validation.
        for ident, params in [
            (40, {"name": "get_item", "arguments": []}),
            (41, {"name": 42, "arguments": {}}),
            (42, {"name": "get_item", "arguments": "OUTER_CANARY"}),
        ]:
            response = client.request(ident, "tools/call", params)
            assert response["error"]["code"] == -32601, response
            assert "OUTER_CANARY" not in json.dumps(response)
        optional = client.request(43, "tools/call", {"name": "list_inventory", "arguments": None})
        assert not optional["result"]["isError"], optional
        required = client.request(44, "tools/call", {"name": "get_item", "arguments": None})
        assert required["error"]["code"] == -32602, required
        resources = client.request(9, "resources/read", {"uri": "file:///SKILL.md"})
        assert resources["error"]["code"] == -32601, resources
        client.request(10, "tools/call", {"name": "list_inventory", "arguments": {}})
        # A real locked cache keeps one filesystem scan busy; protocol and busy
        # responses must remain responsive without accumulating scan workers.
        with sqlite3.connect(root / "state/index.sqlite3") as connection:
            connection.execute("BEGIN IMMEDIATE")
            started = time.monotonic()
            for ident in (20, 21):
                client.send({"jsonrpc": "2.0", "id": ident, "method": "tools/call",
                             "params": {"name": "list_inventory", "arguments": {}}})
            client.send({"jsonrpc": "2.0", "id": 22, "method": "ping"})
            replies = [client.read(), client.read()]
            assert time.monotonic() - started < 2, "blocked scan held the protocol runtime"
            ping = next(r for r in replies if r["id"] == 22)
            assert ping["result"] == {}
            busy = next(r for r in replies if r["id"] != 22)
            assert busy["error"]["code"] == -32603, busy
            connection.rollback()
        completed = client.read()
        assert completed["id"] in (20, 21) and completed["id"] != busy["id"]
        assert not completed["result"]["isError"]
        # Disconnect with a scan in flight while the filesystem/cache is blocked.
        with sqlite3.connect(root / "state/index.sqlite3") as connection:
            connection.execute("BEGIN IMMEDIATE")
            client.send({"jsonrpc":"2.0", "id":30, "method":"tools/call",
                         "params":{"name":"list_inventory", "arguments":{}}})
            assert client.request(31, "ping")["result"] == {}
            started = time.monotonic()
            client.close()
            assert time.monotonic() - started < 2, "in-flight scan delayed disconnect"
            connection.rollback()
    finally:
        client.cleanup()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeat", type=int, choices=range(1, 21), default=1,
                        metavar="1..20", help="bounded live protocol stress repetitions")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]))
    target = Path(metadata["target_directory"])
    for _ in range(args.repeat):
        for version in ("2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25", "1999-01-01"):
            with tempfile.TemporaryDirectory(prefix="m3-protocol-", dir=target) as directory:
                root = Path(directory).resolve()
                (root / "home/.git").mkdir(parents=True)
                (root / "project/.git").mkdir(parents=True)
                scenario(target / "debug/skillrail", root, version)
    print(f"MCP boundary smoke: {args.repeat * 5} sessions, {args.repeat * 80} pipelined malformed requests; versions, limits, errors, EOF passed")


if __name__ == "__main__":
    main()
