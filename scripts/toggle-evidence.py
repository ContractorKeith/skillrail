#!/usr/bin/env python3
"""Inspect pinned native code only; never reads agent configuration or starts a session."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request

CODEX_SHA = "6ff1e1ca3b30fc8b914720681fcf5fe62e95f856"
CLAUDE_LINUX_SHA256 = "fd13c70000fec3f2116dfbcbb6121210b4a29328bf0e46210a59dd594a67d824"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude-binary", type=Path, required=True, help="exact native Claude 2.1.316 executable (not a version-manager wrapper)")
    args = parser.parse_args()
    version = subprocess.check_output([str(args.claude_binary), "--version"], timeout=15, text=True).strip()
    assert version == "2.1.316 (Claude Code)", f"unmet prerequisite: Claude 2.1.316, got {version}"
    code = args.claude_binary.read_bytes()
    digest = hashlib.sha256(code).hexdigest()
    assert digest == CLAUDE_LINUX_SHA256, "unmet prerequisite: this probe pins the recorded Linux native executable; other builds require separate evidence"
    for snippet in [b'skillOverrides:()=>pe(o(),U(["on","name-only","user-invocable-only","off"]))', b'let w=S.disabledMcpServers,L=MY(w),G=KFn(L,n,!r)', b'return{...S,disabledMcpServers:G}']:
        assert snippet in code, "native setting evidence changed"
    base = f"https://raw.githubusercontent.com/openai/codex/{CODEX_SHA}/codex-rs/config/src/"
    sources = {}
    for filename, snippets in {
        "skills_config.rs": [b'pub enabled: bool', b'path.canonicalize().unwrap_or_else', b'entries.retain', b'for entry in skills.config'],
        "mcp_types.rs": [b'pub enabled: bool', b'#[serde(default = "default_enabled")]'],
    }.items():
        with urllib.request.urlopen(base + filename, timeout=20) as response:
            source = response.read()
        for snippet in snippets:
            assert snippet in source, f"pinned source lacks expected evidence: {filename}"
        sources[filename] = {"url": base + filename, "sha256": hashlib.sha256(source).hexdigest()}
    print(json.dumps({"claude_version": version, "claude_linux_sha256": digest, "codex_commit": CODEX_SHA, "source_evidence": sources, "session_loading": "not tested"}, indent=2))


if __name__ == "__main__":
    main()
