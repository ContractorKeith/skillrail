#!/usr/bin/env python3
"""Opt-in real Claude config replay and anonymous HTTPS acquisition evidence.

Never starts a model session, reads user configuration, or emits native output.
Missing safe native session prerequisites remain pending in the release ledger.
"""
import argparse
import hashlib
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import tempfile

if not __debug__:
    raise RuntimeError("Evidence checks require Python assertions; remove -O/PYTHONOPTIMIZE")

ROOT = Path(__file__).resolve().parent.parent
HTTPS_SOURCE = "https://github.com/anthropics/skills.git"
HTTPS_REV = "ca45fc496a3436251f191354091d557da31c15b5"
HTTPS_SKILL = "template"


def run(args, cwd, env, timeout=45):
    with subprocess.Popen([str(a) for a in args], cwd=cwd, env=env,
                          stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, start_new_session=True) as child:
        try:
            stdout, _ = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate()
            raise RuntimeError("command timed out; process group stopped; raw output withheld") from None
    if child.returncode:
        # Native stderr can contain configuration/auth context. Do not print it.
        raise RuntimeError(f"{Path(str(args[0])).name} failed (exit {child.returncode}); raw output withheld")
    return stdout


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(content))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["claude", "https"])
    parser.add_argument("--debug", action="store_true", help="show tracebacks for refusals")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/skillrail")
    parser.add_argument("--claude-binary", type=Path)
    parser.add_argument("--expected-claude-version", default="2.1.316 (Claude Code)")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    # Only these inherited values survive; no provider keys, routing, injected
    # settings, Git wrappers, telemetry endpoints, or token-bearing environment.
    env = {k: os.environ[k] for k in ("HOME", "CODEX_HOME", "TMPDIR") if k in os.environ}
    env.update(PATH="/usr/bin:/bin", LANG="C", GIT_CONFIG_GLOBAL="/dev/null",
               GIT_CONFIG_SYSTEM="/dev/null", GIT_CONFIG_NOSYSTEM="1")
    with tempfile.TemporaryDirectory(prefix="s7n-") as directory:
        root = Path(directory).resolve()
        home, project, state = (root / n for n in ("h", "p", "s"))
        for path in (home / ".claude", home / ".codex", project, state):
            path.mkdir(mode=0o700, parents=True)
        run(["/usr/bin/git", "-c", "core.hooksPath=/dev/null", "init", "--template=", "-q", project], root, env)
        app = [binary, "--home", home, "--project", project, "--state-dir", state]
        commands = []
        if args.mode == "claude":
            if not args.claude_binary or not args.claude_binary.is_absolute():
                parser.error("--claude-binary must name an exact absolute native executable, never a wrapper")
            claude = args.claude_binary.resolve(strict=True)
            env.update(CLAUDE_CONFIG_DIR=str(home / ".claude"),
                       CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_UPDATES="1",
                       DISABLE_AUTOUPDATER="1", DISABLE_TELEMETRY="1")
            version = run([claude, "--version"], project, env).decode().strip()
            assert version == args.expected_claude_version, (
                f"{claude} reports {version!r}, expected {args.expected_claude_version!r}")
            market = root / "market"
            write(market / ".claude-plugin/marketplace.json", {
                "name": "s7-fixture", "owner": {"name": "Fixture"},
                "plugins": [{"name": "inert", "source": "./inert"}]})
            write(market / "inert/.claude-plugin/plugin.json", {"name": "inert", "version": "1.0.0"})
            skill = market / "inert/skills/inert/SKILL.md"
            skill.parent.mkdir(parents=True)
            skill.write_text("---\nname: inert\ndescription: Disposable native release check\n---\nS7_NATIVE_BODY_CANARY\n")
            invocations = [["plugin", "marketplace", "add", str(market), "--json"]]
            invocations += [["plugin", "install", "inert@s7-fixture", "--scope", scope, "--json"] for scope in ("user", "project", "local")]
            for invocation in invocations:
                run([claude, *invocation], project, env)
                commands.append(["claude", *[s.replace(str(root), "FIXTURE") for s in invocation]])
            registry = json.loads((home / ".claude/plugins/installed_plugins.json").read_text())
            assert registry["version"] == 2
            assert {r["scope"] for r in registry["plugins"]["inert@s7-fixture"]} == {"user", "project", "local"}
            raw = run([*app, "scan", "--json"], project, env)
            inventory = json.loads(raw)
            selected = [o for o in inventory["occurrences"] if o["origin"]["kind"] == "plugin" and o["origin"]["installation"]["status"] == "selected"]
            assert len(selected) == len(inventory["occurrences"]) == 3
            assert {o["scope"] for o in selected} == {"user", "project", "local"}
            assert b"S7_NATIVE_BODY_CANARY" not in raw
            result = {"native_version": version, "native_sha256": hashlib.sha256(claude.read_bytes()).hexdigest(),
                      "verified": "real native plugin install records replayed: user/project/local", "session_loading": "pending: safe disposable authenticated session unavailable"}
        else:
            install = [*app, "install", HTTPS_SOURCE, "--git", "--ref", HTTPS_REV, "--skill", HTTPS_SKILL,
                       "--agent", "codex", "--scope", "project", "--apply", "--json"]
            raw = run(install, project, env, timeout=120)
            commands.append([str(a).replace(str(root), "FIXTURE") for a in install])
            inventory_raw = run([*app, "scan", "--json"], project, env)
            inventory = json.loads(inventory_raw)
            assert [i["name"] for i in inventory["items"]] == ["template-skill"]
            assert len(inventory["occurrences"]) == 1
            provenance = inventory["items"][0]["source"]["installed"]
            assert provenance["revision"] == provenance["reference"] == HTTPS_REV
            assert provenance["subpath"] == HTTPS_SKILL
            snapshots = list((home / ".skillrail-sources").rglob("SKILL.md"))
            assert len(snapshots) == 1
            expected = b"---\nname: template-skill\ndescription: Replace with description of the skill and when Claude should use it.\n---\n\n# Insert instructions below\n"
            assert snapshots[0].read_bytes() == expected
            assert b"# Insert instructions below" not in raw + inventory_raw
            result = {"source": HTTPS_SOURCE, "revision": HTTPS_REV, "subpath": HTTPS_SKILL,
                      "skill_sha256": hashlib.sha256(expected).hexdigest(),
                      "verified": "anonymous real HTTPS fetch and applied inert snapshot", "session_loading": "not tested"}
        result.update(commands=commands, isolation="private disposable home/project/state, own Git boundary; HOME/CODEX_HOME unchanged")
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, RuntimeError, OSError) as error:
        if "--debug" in sys.argv:
            raise
        if isinstance(error, OSError) and error.filename:
            message = f"{error.filename}: {error.strerror}"
        else:
            message = " ".join(str(error).split()) or "a verification check failed; rerun with --debug"
        print(f"native-release-evidence.py: {message}", file=sys.stderr)
        sys.exit(1)
