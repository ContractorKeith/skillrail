#!/usr/bin/env python3
"""Inspect pinned native code only; never reads agent configuration or starts a session."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request
import tempfile

CODEX_SHA = "6ff1e1ca3b30fc8b914720681fcf5fe62e95f856"
CLAUDE_LINUX_SHA256 = "fd13c70000fec3f2116dfbcbb6121210b4a29328bf0e46210a59dd594a67d824"


def check_skill_rule_layers(source):
    """Compile the exact pinned resolver with small typed fixture collaborators."""
    text = source.decode()
    start = text.index("pub fn skill_config_rules_from_stack(")
    brace = text.index("{", start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    resolver = text[start:end]
    harness = r'''
#![allow(dead_code, private_interfaces)]
use std::collections::BTreeMap;
macro_rules! warn { ($($arg:tt)*) => { let _ = format_args!($($arg)*); } }
#[derive(Clone, Copy)] enum ConfigLayerSource { User { fixture: () }, SessionFlags, Project, System }
#[derive(Clone)] struct SkillConfig { selector: String, enabled: bool }
#[derive(Clone)] struct SkillsConfig { config: Vec<SkillConfig> }
struct Layer { name: ConfigLayerSource, config: BTreeMap<String, SkillsConfig> }
struct ConfigLayerStack { layers: Vec<Layer> }
impl ConfigLayerStack { fn all_layers_low_to_high(&self) -> std::slice::Iter<'_, Layer> { self.layers.iter() } }
#[derive(Debug, PartialEq, Eq)] struct SkillConfigRule { selector: String, enabled: bool }
struct SkillConfigRules { entries: Vec<SkillConfigRule> }
fn skill_config_rule_selector(rule: &SkillConfig) -> Option<String> { Some(rule.selector.clone()) }
fn layer(name: ConfigLayerSource, selector: &str, enabled: bool) -> Layer {
    Layer { name, config: BTreeMap::from([("skills".into(), SkillsConfig { config: vec![SkillConfig { selector: selector.into(), enabled }] })]) }
}
__RESOLVER__
fn main() {
    let user = ConfigLayerSource::User { fixture: () };
    let stack = ConfigLayerStack { layers: vec![
        layer(ConfigLayerSource::System, "system-only", false),
        layer(user, "demo", false),
        layer(ConfigLayerSource::Project, "demo", true),
        layer(ConfigLayerSource::Project, "project-only", false),
        layer(ConfigLayerSource::SessionFlags, "session-only", false),
    ] };
    let entries = skill_config_rules_from_stack(&stack).entries;
    assert_eq!(entries, vec![SkillConfigRule { selector: "demo".into(), enabled: false }, SkillConfigRule { selector: "session-only".into(), enabled: false }]);
    let stack = ConfigLayerStack { layers: vec![layer(user, "demo", false), layer(ConfigLayerSource::SessionFlags, "demo", true)] };
    assert_eq!(skill_config_rules_from_stack(&stack).entries, vec![SkillConfigRule { selector: "demo".into(), enabled: true }]);
}
'''
    # The native function supplies the policy; fixtures only supply typed layers/data.
    with tempfile.TemporaryDirectory(prefix="sr-layers-") as directory:
        root = Path(directory)
        code = root / "layers.rs"
        binary = root / "layers"
        code.write_text(harness.replace("__RESOLVER__", resolver))
        subprocess.run(["rustc", "--edition=2024", str(code), "-o", str(binary)], check=True, timeout=30, capture_output=True)
        subprocess.run([str(binary)], check=True, timeout=10, capture_output=True)
        # Prove the fixture detects removal of the layer restriction.
        guard_start = resolver.index("        if !matches!(")
        guard_end = resolver.index("        let Some(skills_value)", guard_start)
        unfiltered = resolver[:guard_start] + resolver[guard_end:]
        code.write_text(harness.replace("__RESOLVER__", unfiltered))
        subprocess.run(["rustc", "--edition=2024", str(code), "-o", str(binary)], check=True, timeout=30, capture_output=True)
        assert subprocess.run([str(binary)], timeout=10, capture_output=True).returncode != 0, "layer fixture failed to detect ignored project/system filtering"
    return {"accepted": ["User", "SessionFlags"], "ignored": ["Project", "System"], "session_override": "passed", "removed_filter_negative_control": "failed as expected"}


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
    layers = None
    for filename, snippets in {
        "skills_config.rs": [b'pub enabled: bool', b'path.canonicalize().unwrap_or_else', b'entries.retain', b'for entry in skills.config'],
        "mcp_types.rs": [b'pub enabled: bool', b'#[serde(default = "default_enabled")]'],
    }.items():
        with urllib.request.urlopen(base + filename, timeout=20) as response:
            source = response.read()
        for snippet in snippets:
            assert snippet in source, f"pinned source lacks expected evidence: {filename}"
        if filename == "skills_config.rs":
            layers = check_skill_rule_layers(source)
        sources[filename] = {"url": base + filename, "sha256": hashlib.sha256(source).hexdigest()}
    print(json.dumps({"claude_version": version, "claude_linux_sha256": digest, "codex_commit": CODEX_SHA, "source_evidence": sources, "skill_rule_layer_probe": layers, "session_loading": "not tested"}, indent=2))


if __name__ == "__main__":
    main()
