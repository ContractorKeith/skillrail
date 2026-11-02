use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("home")).unwrap();
    fs::create_dir(root.path().join("project")).unwrap();
    root
}
fn scan(root: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_skillrail"))
        .arg("--home")
        .arg(root.join("home"))
        .arg("--project")
        .arg(root.join("project"))
        .arg("--state-dir")
        .arg(root.join("state"))
        .args(["scan", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn standalone_mcp_keeps_entries_separate_and_explains_status_and_scope() {
    let root = fixture();
    let p = root.path();
    let private = json!({"mcpServers":{"same":{"command":"never-run","args":["CANARY_ARGUMENT"],"env":{"X":"CANARY_ENV"}}},
        "projects":{p.join("project").canonicalize().unwrap().to_str().unwrap():{"mcpServers":{"same":{"type":"http","url":"https://user:CANARY_URL@localhost/secret"}}}}});
    write(p, "home/.claude.json", &private.to_string());
    write(
        p,
        "project/.mcp.json",
        r#"{"mcpServers":{"same":{"type":"sse","url":"http://localhost","headers":{"X":"CANARY_HEADER"}},"broken":{"command":42}}}"#,
    );
    write(
        p,
        "home/.codex/config.toml",
        "[mcp_servers.same]\ncommand = 'never-run'\nenabled = false\n[mcp_servers.on]\nurl = 'https://localhost/CANARY_URL'\n",
    );
    write(
        p,
        "project/.codex/config.toml",
        "[mcp_servers.same]\ncommand = 'never-run'\n",
    );
    let result = scan(p);
    let entries = result["occurrences"].as_array().unwrap();
    assert_eq!(entries.len(), 7);
    assert_eq!(result["items"].as_array().unwrap().len(), 7);
    for (agent, scope, name, status) in [
        ("claude", "user", "same", "enabled"),
        ("claude", "local", "same", "enabled"),
        ("claude", "project", "same", "enabled"),
        ("claude", "project", "broken", "invalid"),
        ("codex", "user", "same", "disabled"),
        ("codex", "project", "same", "enabled"),
    ] {
        let row = entries
            .iter()
            .find(|o| o["agent"] == agent && o["scope"] == scope && o["name"] == name)
            .unwrap();
        assert_eq!(row["mcp"]["configured_status"], status);
        assert_eq!(row["visibility"]["status"], "unknown");
        assert!(
            row["capability"]["reason"]
                .as_str()
                .unwrap()
                .contains("read-only")
        );
        assert!(row["canonical_path"].is_string());
    }
    assert!(result.to_string().contains("precedence"));
    for canary in [
        "CANARY_ARGUMENT",
        "CANARY_ENV",
        "CANARY_URL",
        "CANARY_HEADER",
    ] {
        assert!(!result.to_string().contains(canary));
        assert!(
            !fs::read(p.join("state/index.sqlite3"))
                .unwrap()
                .windows(canary.len())
                .any(|b| b == canary.as_bytes())
        );
    }
}

#[test]
fn cached_plugins_supply_owned_skills_and_mcp_with_unknown_activation() {
    let root = fixture();
    let p = root.path();
    for (agent, folder) in [("claude", ".claude-plugin"), ("codex", ".codex-plugin")] {
        let base = format!("home/.{agent}/plugins/cache/market/demo/1.0");
        write(
            p,
            &format!("{base}/{folder}/plugin.json"),
            r#"{"name":"demo","skills":"./extra/","mcpServers":{"same":{"command":"never-run","args":["CANARY_ARGUMENT"]}}}"#,
        );
        write(
            p,
            &format!("{base}/skills/default/SKILL.md"),
            "---\nname: default\ndescription: Default plugin skill\n---\n",
        );
        write(
            p,
            &format!("{base}/extra/custom/SKILL.md"),
            "---\nname: custom\ndescription: Custom plugin skill\n---\n",
        );
        write(
            p,
            &format!("{base}/.mcp.json"),
            r#"{"mcpServers":{"same":{"type":"http","url":"https://CANARY_URL.invalid"},"other":{"command":"never-run"}}}"#,
        );
    }
    let result = scan(p);
    // Claude adds declared skill roots to default; Codex compatibility manifest selects the declared root.
    assert_eq!(result["occurrences"].as_array().unwrap().len(), 7);
    let rows = result["occurrences"].as_array().unwrap();
    assert!(
        rows.iter()
            .all(|o| o["origin"]["kind"] == "plugin" && o["scope"] == "unknown")
    );
    assert!(rows.iter().all(|o| o["visibility"]["status"] == "unknown"));
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["source"]["owner"] == "native_plugin")
    );
    assert_eq!(
        rows.iter()
            .filter(|o| o["mcp"]["transport"] == "stdio")
            .count(),
        4
    );
    assert!(!result.to_string().contains("CANARY_"));
    // Native source content stays untouched, including credentials and scripts.
    assert!(
        fs::read_to_string(p.join("home/.claude/plugins/cache/market/demo/1.0/.mcp.json"))
            .unwrap()
            .contains("CANARY_URL")
    );
}

#[test]
fn unknown_schema_and_malformed_transport_are_never_positive_status_claims() {
    let root = fixture();
    let p = root.path();
    write(
        p,
        "project/.mcp.json",
        r#"{"mcpServers":{"future":{"type":"future","url":"CANARY_URL"},"wrong":{"type":"stdio","url":"https://localhost"},"badurl":{"type":"http","url":"not a url CANARY_URL"},"extra":{"command":"never-run","futureMode":"CANARY_ENV"}}}"#,
    );
    write(
        p,
        "home/.codex/config.toml",
        "[mcp_servers.wrong]\ncommand = 'never-run'\nenabled = 'CANARY_ENV'\n",
    );
    let result = scan(p);
    let rows = result["occurrences"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    for (name, status) in [
        ("future", "unknown"),
        ("wrong", "invalid"),
        ("badurl", "invalid"),
        ("extra", "unknown"),
    ] {
        for row in rows.iter().filter(|o| o["name"] == name) {
            assert_eq!(row["mcp"]["configured_status"], status, "{row}");
        }
    }
    assert!(!result.to_string().contains("CANARY_"));
}

#[test]
fn mcp_cli_tui_and_cache_share_safe_details_without_execution_or_network() {
    use ratatui::{Terminal, backend::TestBackend};
    use skillrail::{Application, Paths, tui};
    use std::net::TcpListener;
    let root = fixture();
    let p = root.path();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let marker = p.join("EXECUTED");
    let document = json!({"mcpServers":{
        "execute":{"command":"sh","args":["-c",format!("touch {}",marker.display()),"CANARY_ARGUMENT"],"env":{"X":"CANARY_ENV"},"headersHelper":"CANARY_HELPER"},
        "remote":{"type":"http","url":format!("http://{}/CANARY_URL",listener.local_addr().unwrap()),"headers":{"X":"CANARY_HEADER"}}
    }}).to_string();
    write(p, "home/.claude.json", &document);
    write(
        p,
        "home/.codex/config.toml",
        &format!(
            "[mcp_servers.command]\ncommand = 'sh'\nargs = ['-c', 'touch {}', 'CANARY_ARGUMENT']\nenv = {{ X = 'CANARY_ENV' }}\n[mcp_servers.remote]\nurl = 'http://{}/CANARY_URL'\nhttp_headers = {{ X = 'CANARY_HEADER' }}\n",
            marker.display(),
            listener.local_addr().unwrap()
        ),
    );
    for (agent, manifest) in [("claude", ".claude-plugin"), ("codex", ".codex-plugin")] {
        let base = format!("home/.{agent}/plugins/cache/market/canary/1");
        write(
            p,
            &format!("{base}/{manifest}/plugin.json"),
            r#"{"name":"canary","mcpServers":{"secret":{"command":"never-run","args":["CANARY_ARGUMENT"],"env":{"X":"CANARY_ENV"}}}}"#,
        );
        write(
            p,
            &format!("{base}/.mcp.json"),
            r#"{"mcpServers":{"remote":{"type":"http","url":"https://localhost/CANARY_URL","headers":{"X":"CANARY_HEADER"}}}}"#,
        );
    }
    let before = snapshot(p);
    let result = scan(p);
    assert_eq!(snapshot(p), before);
    assert!(!marker.exists());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        fs::read_to_string(p.join("home/.claude.json")).unwrap(),
        document
    );
    let app = Application::new(
        Paths::resolve(
            Some(p.join("home")),
            Some(p.join("project")),
            Some(p.join("state")),
        )
        .unwrap(),
    );
    let inventory = app.scan().unwrap();
    let mut value = serde_json::to_value(&inventory).unwrap();
    let mut cli = result;
    value.as_object_mut().unwrap().remove("scanned_at_unix_ms");
    cli.as_object_mut().unwrap().remove("scanned_at_unix_ms");
    assert_eq!(value, cli);
    let mut terminal = Terminal::new(TestBackend::new(180, 70)).unwrap();
    for selected in 0..inventory.items.len() {
        terminal
            .draw(|f| tui::render_selection(f, &inventory, selected, 0))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Configured MCP:"));
        assert!(text.contains("read-only"));
        assert!(!text.contains("CANARY_"));
        assert!(!text.contains('\u{1b}'));
    }
    let bytes = fs::read(p.join("state/index.sqlite3")).unwrap();
    assert!(!bytes.windows(7).any(|s| s == b"CANARY_"));
    // Malformed input can contain a credential in the parser's context; never echo it.
    write(p, "home/.claude.json", "{CANARY_PARSE");
    let malformed = scan(p);
    assert!(malformed.to_string().contains("mcp_config_unknown"));
    assert!(!malformed.to_string().contains("CANARY_"));
}

#[test]
fn plugin_selection_observations_preserve_unknown_scope_and_runtime_visibility() {
    let root = fixture();
    let p = root.path();
    for (agent, manifest) in [("claude", ".claude-plugin"), ("codex", ".codex-plugin")] {
        write(
            p,
            &format!("home/.{agent}/plugins/cache/market/demo/1/{manifest}/plugin.json"),
            r#"{"name":"demo","mcpServers":{"same":{"command":"never-run"}}}"#,
        );
    }
    write(
        p,
        "home/.codex/config.toml",
        "[plugins.'demo@market']\nenabled = false\n",
    );
    write(
        p,
        "home/.claude/settings.json",
        r#"{"enabledPlugins":{"demo@market":true}}"#,
    );
    let result = scan(p);
    let entries = result["occurrences"].as_array().unwrap();
    for (agent, status) in [("claude", "enabled"), ("codex", "disabled")] {
        let row = entries.iter().find(|o| o["agent"] == agent).unwrap();
        assert_eq!(row["origin"]["configured_status"], status);
        assert_eq!(row["scope"], "unknown");
        assert_eq!(row["visibility"]["status"], "unknown");
    }
    // A project observation is not automatically a trusted override.
    write(
        p,
        "project/.codex/config.toml",
        "[plugins.'demo@market']\nenabled = true\n",
    );
    let result = scan(p);
    assert_eq!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["agent"] == "codex")
            .unwrap()["origin"]["configured_status"],
        "unknown"
    );
}

#[test]
fn claude_remote_requires_explicit_transport_and_cannot_reuse_codex_schema() {
    let root = fixture();
    write(
        root.path(),
        "project/.mcp.json",
        r#"{"mcpServers":{"missing":{"url":"https://localhost"},"portable":{"type":"streamable-http","url":"https://localhost"}}}"#,
    );
    let result = scan(root.path());
    assert!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["mcp"]["configured_status"] == "invalid")
    );
}

#[test]
fn portable_plugins_and_unsupported_components_are_explicit_and_contained() {
    let root = fixture();
    let p = root.path();
    let base = "home/.codex/plugins/cache/market/portable/1";
    write(
        p,
        &format!("{base}/plugin.json"),
        r#"{"name":"portable","extensions":{"com.openai":{}}}"#,
    );
    write(
        p,
        &format!("{base}/.codex-plugin/plugin.json"),
        "{CANARY_PARSE",
    );
    write(
        p,
        &format!("{base}/mcp.json"),
        r#"{"mcpServers":{"portable":{"type":"streamable-http","url":"https://localhost/CANARY_URL"}}}"#,
    );
    write(
        p,
        &format!("{base}/skills/portable/SKILL.md"),
        "---\nname: portable\ndescription: Portable skill\n---\n",
    );
    let claude = "home/.claude/plugins/cache/market/bundle/1";
    write(
        p,
        &format!("{claude}/.claude-plugin/plugin.json"),
        r#"{"name":"bundle","skills":"./../../../../../../outside","mcpServers":[{"same":{"command":"never-run"}},"https://CANARY_URL.invalid/server.mcpb"]}"#,
    );
    write(
        p,
        "outside/escape/SKILL.md",
        "---\nname: escape\ndescription: Must not be scanned\n---\n",
    );
    let result = scan(p);
    assert_eq!(result["items"].as_array().unwrap().len(), 3);
    assert!(result.to_string().contains("plugin_component_unknown"));
    assert!(!result.to_string().contains("CANARY_"));
    assert!(
        !result["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["name"] == "escape")
    );
    let bundle = result["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["agent"] == "claude")
        .unwrap();
    assert_eq!(bundle["mcp"]["configured_status"], "unknown");
}

#[test]
fn custom_plugin_json_merges_in_order_and_never_follows_escaping_manifest() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/market/demo/1";
    write(
        p,
        &format!("{base}/.claude-plugin/plugin.json"),
        r#"{"name":"demo","mcpServers":["./one.json","./two.json"]}"#,
    );
    write(
        p,
        &format!("{base}/one.json"),
        r#"{"same":{"command":"never-run"}}"#,
    );
    write(
        p,
        &format!("{base}/two.json"),
        r#"{"mcpServers":{"same":{"type":"http","url":"https://localhost/CANARY_URL"}}}"#,
    );
    let result = scan(p);
    assert_eq!(result["occurrences"].as_array().unwrap().len(), 1);
    assert_eq!(result["occurrences"][0]["mcp"]["transport"], "http");
    assert_eq!(
        result["occurrences"][0]["mcp"]["configured_status"],
        "enabled"
    );
    #[cfg(unix)]
    {
        let evil = p.join("home/.codex/plugins/cache/market/evil/1");
        fs::create_dir_all(&evil).unwrap();
        write(
            p,
            "outside/plugin.json",
            r#"{"name":"evil","mcpServers":{"escaped":{"command":"never-run"}}}"#,
        );
        std::os::unix::fs::symlink(p.join("outside"), evil.join(".codex-plugin")).unwrap();
        let result = scan(p);
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        assert!(result.to_string().contains("plugin_manifest_unknown"));
    }
}

fn snapshot(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(path: &Path, result: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(&entry.unwrap().path(), result);
            }
        } else {
            result.insert(path.into(), fs::read(path).unwrap());
        }
    }
    let mut result = std::collections::BTreeMap::new();
    visit(&root.join("home"), &mut result);
    visit(&root.join("project"), &mut result);
    result
}

#[test]
fn malformed_plugin_rules_do_not_hide_native_owned_occurrences() {
    let root = fixture();
    let p = root.path();
    write(
        p,
        "home/.codex/plugins/cache/market/demo/1/.codex-plugin/plugin.json",
        r#"{"name":"demo","mcpServers":{"same":{"command":"never-run"}}}"#,
    );
    write(
        p,
        "home/.codex/config.toml",
        "[plugins.'demo@market']\nenabled = 'CANARY_PARSE'\n",
    );
    let result = scan(p);
    assert_eq!(result["occurrences"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["occurrences"][0]["origin"]["configured_status"],
        "invalid"
    );
    assert!(
        result["occurrences"][0]["diagnostics"]
            .to_string()
            .contains("plugin_rule_invalid")
    );
    assert!(!result.to_string().contains("CANARY_"));
}

#[test]
fn claude_skills_directory_plugins_keep_user_project_scope_without_parent_discovery() {
    let root = fixture();
    let p = root.path();
    fs::create_dir(p.join("project/.git")).unwrap();
    for (base, name) in [
        ("home", "personal"),
        ("project", "ancestor"),
        ("project/nested", "selected"),
    ] {
        let base = format!("{base}/.claude/skills/{name}");
        write(
            p,
            &format!("{base}/.claude-plugin/plugin.json"),
            &format!(r#"{{"name":"{name}","mcpServers":{{"{name}":{{"command":"never-run"}}}}}}"#),
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_skillrail"))
        .arg("--home")
        .arg(p.join("home"))
        .arg("--project")
        .arg(p.join("project/nested"))
        .arg("--state-dir")
        .arg(p.join("state"))
        .args(["scan", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = result["occurrences"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .any(|o| o["name"] == "personal" && o["scope"] == "user")
    );
    assert!(
        entries
            .iter()
            .any(|o| o["name"] == "selected" && o["scope"] == "project")
    );
    assert!(!entries.iter().any(|o| o["name"] == "ancestor"));
}

#[test]
fn standalone_skill_aliases_are_not_misdiagnosed_as_unsupported_plugins() {
    let root = fixture();
    let p = root.path();
    write(
        p,
        "home/.claude/skills/real/SKILL.md",
        "---\nname: real\ndescription: Fixture\n---\n",
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink("real", p.join("home/.claude/skills/alias")).unwrap();
    let result = scan(p);
    assert!(
        !result["diagnostics"]
            .to_string()
            .contains("plugin_link_unsupported")
    );
}

#[test]
fn mcp_names_do_not_change_standalone_skill_collision_facts() {
    let root = fixture();
    let p = root.path();
    write(
        p,
        "home/.agents/skills/same/SKILL.md",
        "---\nname: same\ndescription: Fixture\n---\n",
    );
    write(
        p,
        "home/.codex/config.toml",
        "[mcp_servers.same]\ncommand = 'never-run'\n",
    );
    let result = scan(p);
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["name_collision"] == false)
    );
}

#[test]
fn unsupported_server_keys_are_redacted_instead_of_retained_as_safe_labels() {
    let root = fixture();
    write(
        root.path(),
        "project/.mcp.json",
        r#"{"mcpServers":{"bad:CANARY_KEY":{"command":"never-run"}}}"#,
    );
    let result = scan(root.path());
    assert_eq!(result["occurrences"][0]["name"], "unsupported-name");
    assert_eq!(
        result["occurrences"][0]["mcp"]["configured_status"],
        "invalid"
    );
    assert!(!result.to_string().contains("CANARY_"));
}
