#!/usr/bin/env python3
"""Optional native schema probe. Uses a disposable local marketplace and config home."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


def main():
    assert subprocess.check_output(["claude", "--version"], text=True).strip() == "2.1.309 (Claude Code)"
    scratch = Path("target/m2-evidence")
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="native-", dir=scratch) as tmp:
        root = Path(tmp).resolve()
        home, project, state = [root / name for name in ("home", "project", "state")]
        market = home / "market"
        for directory in (home / ".claude", project, state):
            directory.mkdir(parents=True)
        names = ("personal", "shared", "private")
        for name in names:
            write(market / name / ".claude-plugin/plugin.json", {"name": name, "version": "1.2.3"})
            (market / name / "SKILL.md").write_text(f"---\nname: {name}\ndescription: Native fixture\n---\n")
        write(market / ".claude-plugin/marketplace.json", {
            "name": "fixture-market", "owner": {"name": "Fixture"},
            "plugins": [{"name": name, "source": "./" + name} for name in names],
        })
        # HOME and CODEX_HOME retain their inherited values. Claude's documented
        # config-directory override isolates all native configuration writes.
        env = {key: value for key, value in os.environ.items()
               if not any(part in key.upper() for part in ("TOKEN", "SECRET", "PASSWORD", "API_KEY", "AUTH"))}
        env.update(CLAUDE_CONFIG_DIR=str(home / ".claude"),
                   CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1",
                   DISABLE_AUTOUPDATER="1", DISABLE_TELEMETRY="1")

        def cli(*args):
            result = subprocess.run(["claude", *args], cwd=project, env=env,
                                    capture_output=True, text=True, timeout=40)
            print("COMMAND", args, "EXIT", result.returncode)
            print(result.stdout.replace(str(root), "FIXTURE"))
            print(result.stderr.replace(str(root), "FIXTURE"))
            assert result.returncode == 0

        cli("plugin", "marketplace", "add", str(market), "--json")
        for name, scope in (("personal", "user"), ("shared", "project"), ("private", "local")):
            cli("plugin", "install", name + "@fixture-market", "--scope", scope, "--json")
        cli("plugin", "install", "personal@fixture-market", "--scope", "project", "--json")
        write(market / "personal/.claude-plugin/plugin.json", {"name": "personal", "version": "2.0.0"})
        cli("plugin", "update", "personal@fixture-market", "--scope", "project", "--json")
        registry = home / ".claude/plugins/installed_plugins.json"
        print("REGISTRY", registry.read_text().replace(str(root), "FIXTURE"))
        contents = json.loads(registry.read_text())
        assert contents["version"] == 2
        assert {(r["scope"], r["version"]) for r in contents["plugins"]["personal@fixture-market"]} == {
            ("user", "1.2.3"), ("project", "2.0.0")}
        binary = Path("target/debug/skillrail").resolve()
        result = subprocess.run([str(binary), "--home", str(home), "--project", str(project),
                                 "--state-dir", str(state), "scan", "--json"],
                                capture_output=True, check=True, text=True)
        inventory = json.loads(result.stdout)
        selected = [o for o in inventory["occurrences"]
                    if o["origin"]["kind"] == "plugin" and o["origin"]["installation"]["status"] == "selected"]
        assert len(selected) == 4, inventory
        assert {o["scope"] for o in selected} == {"user", "project", "local"}
        print("Native registry replay: four selected skill occurrences across user/project/local passed")


if __name__ == "__main__":
    main()
