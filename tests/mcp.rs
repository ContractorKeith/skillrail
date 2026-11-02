use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[test]
fn installed_manifestless_claude_plugin_keeps_default_components() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/m/p/1";
    write(
        p,
        &format!("{base}/skills/example/SKILL.md"),
        "---\nname: example\ndescription: Fixture\n---\n",
    );
    write(
        p,
        &format!("{base}/.mcp.json"),
        r#"{"mcpServers":{"server":{"command":"never"}}}"#,
    );
    write(p, "home/.claude/plugins/installed_plugins.json", &json!({"version":2,"plugins":{"p@m":[{"scope":"user","installPath":p.join(base),"version":"1"}]}}).to_string());
    let before = snapshot(p);
    let result = scan(p);
    let rows = result["occurrences"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|o| o["scope"] == "user" && o["origin"]["installation"]["status"] == "selected")
    );
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["source"]["owner"] == "native_plugin")
    );
    assert_eq!(snapshot(p), before);
    write(p, &format!("{base}/.claude-plugin/plugin.json"), "{");
    let malformed = scan(p);
    assert!(malformed["occurrences"].as_array().unwrap().is_empty());
    assert!(
        malformed["diagnostics"]
            .to_string()
            .contains("plugin_manifest_unknown")
    );
}

#[test]
fn skill_root_enumeration_consumes_shared_plugin_entry_budget() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/m/p/1";
    write(
        p,
        &format!("{base}/.claude-plugin/plugin.json"),
        r#"{"name":"p","skills":["./first","./second"],"mcpServers":{"server":{"command":"never"}}}"#,
    );
    for dir in ["first", "second"] {
        for n in 0..600 {
            write(p, &format!("{base}/{dir}/file{n}"), "");
        }
    }
    let result = scan(p);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == "plugin_budget_exceeded")
            .count(),
        1
    );
    assert!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| !o["mcp"].is_null())
            .all(|o| o["mcp"]["configured_status"] == "unknown")
    );
}

#[test]
fn codex_transport_fields_and_millisecond_types_match_native_schema() {
    let root = fixture();
    let p = root.path();
    for (fields, status) in [
        (
            "command='never'\nhttp_headers={X='CANARY_HEADER'}",
            "invalid",
        ),
        (
            "url='https://example.invalid'\nargs=['CANARY_ARG']",
            "invalid",
        ),
        ("command='never'\nstartup_timeout_ms=0.5", "invalid"),
        ("command='never'\nstartup_timeout_ms=5", "enabled"),
        ("command='never'\nargs=['CANARY_ARG']", "enabled"),
        (
            "url='https://example.invalid'\nhttp_headers={X='CANARY_HEADER'}",
            "enabled",
        ),
    ] {
        write(
            p,
            "home/.codex/config.toml",
            &format!("[mcp_servers.server]\n{fields}\n"),
        );
        let result = scan(p);
        assert_eq!(
            result["occurrences"][0]["mcp"]["configured_status"], status,
            "{fields}"
        );
        assert!(!result.to_string().contains("CANARY_"));
    }
}

#[test]
fn conflicting_versions_in_one_installation_scope_are_unknown() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/m/p";
    for version in ["1", "2"] {
        write(
            p,
            &format!("{base}/{version}/.claude-plugin/plugin.json"),
            r#"{"name":"p","mcpServers":{"server":{"command":"never"}}}"#,
        );
    }
    let records: Vec<_> = ["1", "2"].into_iter().map(|version| json!({"scope":"user","installPath":p.join(base).join(version),"version":version})).collect();
    write(
        p,
        "home/.claude/plugins/installed_plugins.json",
        &json!({"version":2,"plugins":{"p@m":records}}).to_string(),
    );
    let result = scan(p);
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "plugin_registry_unknown")
    );
    assert!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["origin"]["installation"]["status"] == "unknown")
    );
}

#[test]
fn fingerprint_reads_consume_shared_plugin_read_budget() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/m/p/1";
    write(
        p,
        &format!("{base}/.claude-plugin/plugin.json"),
        r#"{"name":"p","mcpServers":{"server":{"command":"never"}}}"#,
    );
    write(
        p,
        &format!("{base}/skills/example/SKILL.md"),
        "---\nname: example\ndescription: Fixture\n---\n",
    );
    for n in 0..300 {
        write(p, &format!("{base}/skills/example/file{n}"), "x");
    }
    let result = scan(p);
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "plugin_budget_exceeded")
    );
    let rows = result["occurrences"].as_array().unwrap();
    assert!(
        rows.iter()
            .filter(|o| o["mcp"].is_null())
            .all(|o| o["fingerprint"]["sha256"].is_null())
    );
    assert!(
        rows.iter()
            .filter(|o| !o["mcp"].is_null())
            .all(|o| o["mcp"]["configured_status"] == "unknown")
    );
}

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

#[test]
fn claude_registry_selects_scoped_installations_not_residual_or_other_projects() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/market/demo";
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        write(
            p,
            &format!("{base}/{version}/.claude-plugin/plugin.json"),
            r#"{"name":"demo","mcpServers":{"same":{"command":"never-run"}}}"#,
        );
    }
    let registry = json!({"version":2,"plugins":{"demo@market":[
        {"scope":"user","installPath":p.join(base).join("2.0.0"),"version":"2.0.0"},
        {"scope":"project","projectPath":p.join("project").canonicalize().unwrap(),"installPath":p.join(base).join("2.0.0"),"version":"2.0.0"},
        {"scope":"local","projectPath":p.join("project").canonicalize().unwrap(),"installPath":p.join(base).join("2.0.0"),"version":"2.0.0","gitCommitSha":"CANARY_REGISTRY_SECRET"},
        {"scope":"local","projectPath":p.join("other"),"installPath":p.join(base).join("3.0.0"),"version":"3.0.0"}
    ]}});
    write(
        p,
        "home/.claude/plugins/installed_plugins.json",
        &registry.to_string(),
    );
    let result = scan(p);
    let rows = result["occurrences"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|o| o["origin"]["installation"]["status"] == "selected")
            .count(),
        3
    );
    assert!(!result.to_string().contains("CANARY_REGISTRY_SECRET"));
    for scope in ["user", "project", "local"] {
        assert!(
            rows.iter()
                .any(|o| o["scope"] == scope && o["origin"]["installation"]["version"] == "2.0.0")
        );
    }
    assert_eq!(
        rows.iter()
            .filter(|o| o["origin"]["installation"]["status"] == "residual")
            .count(),
        2
    );
    write(
        p,
        "home/.claude/plugins/installed_plugins.json",
        r#"{"version":999,"plugins":{},"CANARY_REGISTRY":"secret"}"#,
    );
    let result = scan(p);
    assert!(
        result["diagnostics"]
            .to_string()
            .contains("plugin_registry_unknown")
    );
    assert!(!result.to_string().contains("CANARY_REGISTRY"));
    assert!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["origin"]["installation"]["status"] == "unknown")
    );
    for malformed in [
        r#"{"version":999,"plugins":{}}"#,
        r#"{"version":2,"plugins":"CANARY_REGISTRY"}"#,
        r#"{"version":2,"plugins":{"demo@market":[{"scope":"user","installPath":42,"version":"CANARY_REGISTRY"}]}}"#,
    ] {
        write(p, "home/.claude/plugins/installed_plugins.json", malformed);
        let result = scan(p);
        assert!(
            result["diagnostics"]
                .to_string()
                .contains("plugin_registry_unknown")
        );
        assert!(!result.to_string().contains("CANARY_REGISTRY"));
    }
}

#[test]
fn codex_installation_selection_uses_native_version_order_and_versioned_identity_metadata() {
    let root = fixture();
    let p = root.path();
    let base = "home/.codex/plugins/cache/market/demo";
    for version in ["1.9.0", "1.10.0", "1.10.0-rc.1"] {
        write(
            p,
            &format!("{base}/{version}/.codex-plugin/plugin.json"),
            r#"{"name":"demo","mcpServers":{"same":{"command":"never-run"}}}"#,
        );
    }
    let result = scan(p);
    let selected = result["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["origin"]["installation"]["status"] == "selected")
        .unwrap();
    assert_eq!(selected["origin"]["installation"]["version"], "1.10.0");
    write(
        p,
        &format!("{base}/.codex-remote-plugin-install.json"),
        r#"{"schema_version":999,"remote_plugin_id":"CANARY_REMOTE"}"#,
    );
    let result = scan(p);
    assert!(
        result["diagnostics"]
            .to_string()
            .contains("plugin_install_metadata_unknown")
    );
    assert!(!result.to_string().contains("CANARY_REMOTE"));
    write(
        p,
        &format!("{base}/.codex-remote-plugin-install.json"),
        r#"{"schema_version":1,"remote_plugin_id":"CANARY_REMOTE","new_policy":"CANARY_POLICY"}"#,
    );
    let result = scan(p);
    assert!(
        result["diagnostics"]
            .to_string()
            .contains("plugin_install_metadata_unknown")
    );
    assert!(!result.to_string().contains("CANARY_"));
}

#[test]
fn persisted_claude_choices_retain_source_scope_and_disable_observations() {
    let root = fixture();
    let p = root.path();
    write(p,"home/.claude.json",&json!({"mcpServers":{"personal":{"command":"never"}},"projects":{p.join("project").canonicalize().unwrap().to_str().unwrap():{"disabledMcpServers":["personal"]}}}).to_string());
    write(
        p,
        "project/.mcp.json",
        r#"{"mcpServers":{"shared":{"command":"never"}}}"#,
    );
    write(
        p,
        "project/.claude/settings.local.json",
        r#"{"disabledMcpjsonServers":["shared"],"unrelated":"CANARY_LAYER"}"#,
    );
    let result = scan(p);
    let rows = result["occurrences"].as_array().unwrap();
    for (name, scope, setting) in [
        ("personal", "local", "disabledMcpServers"),
        ("shared", "local", "disabledMcpjsonServers"),
    ] {
        let row = rows.iter().find(|o| o["name"] == name).unwrap();
        assert_eq!(row["mcp"]["configured_status"], "disabled");
        assert!(
            row["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["scope"] == scope
                    && o["setting"] == setting
                    && o["status"] == "disabled"
                    && o["source_path"].is_string())
        );
        assert_eq!(row["visibility"]["status"], "unknown");
    }
    assert!(!result.to_string().contains("CANARY_LAYER"));
}
#[test]
fn codex_plugin_layer_facts_survive_unrelated_and_malformed_project_configuration() {
    let root = fixture();
    let p = root.path();
    write(
        p,
        "home/.codex/plugins/cache/m/p/1/.codex-plugin/plugin.json",
        r#"{"name":"p","mcpServers":{"same":{"command":"never"}}}"#,
    );
    write(
        p,
        "home/.codex/config.toml",
        "[plugins.'p@m']\nenabled=true\n[plugins.'p@m'.mcp_servers.same]\nenabled=false\n",
    );
    write(p, "project/.codex/config.toml", "model='example'\n");
    let result = scan(p);
    let row = &result["occurrences"][0];
    assert_eq!(row["origin"]["configured_status"], "enabled");
    assert_eq!(row["mcp"]["configured_status"], "disabled");
    assert!(
        row["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["scope"] == "user"
                && o["setting"] == "plugin_mcp_enabled"
                && o["status"] == "disabled")
    );
    write(
        p,
        "project/.codex/config.toml",
        "[plugins.'p@m']\nmcp_servers='PRIVATE_CANARY'\n",
    );
    let result = scan(p);
    let row = &result["occurrences"][0];
    assert!(matches!(
        row["mcp"]["configured_status"].as_str(),
        Some("unknown" | "invalid")
    ));
    assert!(
        row["diagnostics"]
            .to_string()
            .contains("plugin_rule_invalid")
    );
    assert!(
        row["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["scope"] == "user" && o["status"] == "disabled")
    );
    assert!(!result.to_string().contains("PRIVATE_CANARY"));
    assert!(
        !fs::read(p.join("state/index.sqlite3"))
            .unwrap()
            .windows(14)
            .any(|w| w == b"PRIVATE_CANARY")
    );
    let inventory = skillrail::Application::new(
        skillrail::Paths::resolve(
            Some(p.join("home")),
            Some(p.join("project")),
            Some(p.join("state")),
        )
        .unwrap(),
    )
    .scan()
    .unwrap();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(200, 100)).unwrap();
    terminal
        .draw(|f| skillrail::tui::render_selection(f, &inventory, 0, 0))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(!text.contains("PRIVATE_CANARY"));
    assert!(text.contains("plugin_rule_invalid"));
    for malformed in [
        "[plugins.'p@m'.mcp_servers]\nsame='PRIVATE_CANARY'",
        "[plugins.'p@m'.mcp_servers.same]\nenabled='PRIVATE_CANARY'",
        "[plugins.'p@m'.mcp_servers.same]\nenabled_tools=42",
    ] {
        write(p, "project/.codex/config.toml", malformed);
        let result = scan(p);
        assert_eq!(
            result["occurrences"][0]["mcp"]["configured_status"],
            "invalid"
        );
        assert!(
            result["occurrences"][0]["diagnostics"]
                .to_string()
                .contains("plugin_rule_invalid")
        );
        assert!(!result.to_string().contains("PRIVATE_CANARY"));
    }
    write(p, "project/.codex/config.toml", "model='example'");
    write(
        p,
        "home/.codex/config.toml",
        "[plugins.'p@m']\nenabled=true\nmcp_servers='PRIVATE_CANARY'\n",
    );
    let result = scan(p);
    assert_eq!(
        result["occurrences"][0]["mcp"]["configured_status"],
        "invalid"
    );
    assert!(
        result["occurrences"][0]["diagnostics"]
            .to_string()
            .contains("plugin_rule_invalid")
    );
    assert!(!result.to_string().contains("PRIVATE_CANARY"));
}

#[test]
fn claude_root_plugin_skill_is_one_owned_occurrence_without_child_traversal() {
    let root = fixture();
    let p = root.path();
    for base in [
        "home/.claude/skills/p",
        "project/.claude/skills/p",
        "home/.claude/plugins/cache/m/p/1",
    ] {
        write(
            p,
            &format!("{base}/.claude-plugin/plugin.json"),
            r#"{"name":"p"}"#,
        );
        write(
            p,
            &format!("{base}/SKILL.md"),
            "---\nname: root-skill\ndescription: Fixture\n---\n",
        );
        write(
            p,
            &format!("{base}/unrelated/SKILL.md"),
            "---\nname: unexpected-child\ndescription: Must not be enumerated\n---\n",
        );
    }
    let result = scan(p);
    let rows = result["occurrences"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|o| o["name"] == "root-skill" && o["origin"]["kind"] == "plugin")
    );
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["source"]["owner"] == "native_plugin")
    );
    write(
        p,
        "home/.claude/skills/p/.claude-plugin/plugin.json",
        r#"{"name":"p","skills":[]}"#,
    );
    assert_eq!(scan(p)["occurrences"].as_array().unwrap().len(), 2);
}

#[test]
fn codex_documented_field_forms_validate_without_exposing_environment_selectors() {
    let root = fixture();
    let p = root.path();
    for (fields, status) in [
        ("default_tools_approval_mode='banana'", "invalid"),
        ("scopes=42", "invalid"),
        (
            "env_vars=[{name='CANARY_ENV_NAME',source='local'}]",
            "enabled",
        ),
        (
            "env_vars=[{name='CANARY_ENV_NAME',source='remote'}]",
            "enabled",
        ),
        (
            "env_vars=[{name='CANARY_ENV_NAME',source='banana'}]",
            "invalid",
        ),
        ("env_vars=[{name=42,source='local'}]", "invalid"),
        (
            "env_vars=[{name='CANARY_ENV_NAME',source='local',future='CANARY_EXTRA'}]",
            "invalid",
        ),
        (
            "default_tools_approval_mode='writes'\nscopes=['CANARY_SCOPE']",
            "enabled",
        ),
    ] {
        write(
            p,
            "home/.codex/config.toml",
            &format!("[mcp_servers.same]\ncommand='never'\n{fields}\n"),
        );
        let result = scan(p);
        assert_eq!(
            result["occurrences"][0]["mcp"]["configured_status"], status,
            "{fields}"
        );
        assert!(!result.to_string().contains("CANARY_"));
    }
}

#[test]
fn plugin_loading_has_cumulative_entry_declaration_and_byte_budgets() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/m/p/1";
    let mut declarations = vec![];
    for file in 0..12 {
        let name = format!("./part{file}.json");
        declarations.push(name);
        let entries: serde_json::Map<_, _> = (0..100)
            .map(|n| {
                (
                    format!("server{file}_{n}"),
                    json!({"command":"never","env":{"X":"CANARY_BUDGET"}}),
                )
            })
            .collect();
        write(
            p,
            &format!("{base}/part{file}.json"),
            &json!({"mcpServers":entries}).to_string(),
        );
    }
    write(
        p,
        &format!("{base}/.claude-plugin/plugin.json"),
        &json!({"name":"p","mcpServers":declarations}).to_string(),
    );
    let before = snapshot(p);
    let result = scan(p);
    assert!(result["items"].as_array().unwrap().len() <= 1000);
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "plugin_budget_exceeded")
    );
    assert!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["mcp"]["configured_status"] == "unknown")
    );
    assert_eq!(snapshot(p), before);
    assert!(!result.to_string().contains("CANARY_BUDGET"));
    // Repeated empty files and nested arrays consume declarations/reads, not just unique entries.
    write(p, &format!("{base}/empty.json"), r#"{"mcpServers":{}}"#);
    write(
        p,
        &format!("{base}/.claude-plugin/plugin.json"),
        &json!({"name":"p","mcpServers":[{"first":{"command":"never"}},vec!["./empty.json";300]]})
            .to_string(),
    );
    let result = scan(p);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == "plugin_budget_exceeded")
            .count(),
        1
    );
    assert_eq!(
        result["occurrences"][0]["mcp"]["configured_status"],
        "unknown"
    );
}

#[test]
fn duplicate_configuration_keys_are_unknown_without_guessing_native_precedence() {
    let root = fixture();
    let p = root.path();
    write(
        p,
        "project/.mcp.json",
        r#"{"mcpServers":{"same":{"command":"never"},"same":{"command":"CANARY_DUPLICATE"}}}"#,
    );
    let result = scan(p);
    assert!(result["items"].as_array().unwrap().is_empty());
    assert!(
        result["diagnostics"]
            .to_string()
            .contains("mcp_config_unknown")
    );
    assert!(!result.to_string().contains("CANARY_DUPLICATE"));
}

#[test]
fn plugin_read_bytes_and_candidate_limits_apply_across_the_scan() {
    let root = fixture();
    let p = root.path();
    let base = "home/.claude/plugins/cache/m/p/1";
    let declarations: Vec<_> = (0..12).map(|n| format!("./part{n}.json")).collect();
    for n in 0..12 {
        write(p,&format!("{base}/part{n}.json"),&json!({"mcpServers":{format!("server{n}"):{"command":"never","env":{"X":"CANARY_BYTES".repeat(9000)}}}}).to_string());
    }
    write(
        p,
        &format!("{base}/.claude-plugin/plugin.json"),
        &json!({"name":"p","mcpServers":declarations}).to_string(),
    );
    let result = scan(p);
    assert!(result["items"].as_array().unwrap().len() < 12);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == "plugin_budget_exceeded")
            .count(),
        1
    );
    assert!(!result.to_string().contains("CANARY_BYTES"));
    // The same scan-wide entry budget applies when no individual plugin exceeds it.
    fs::remove_dir_all(p.join("home/.claude/plugins/cache")).unwrap();
    for plugin in 0..12 {
        let entries: serde_json::Map<_, _> = (0..100)
            .map(|n| (format!("server{n}"), json!({"command":"never"})))
            .collect();
        write(
            p,
            &format!("home/.claude/plugins/cache/m/p{plugin}/1/.claude-plugin/plugin.json"),
            &json!({"name":format!("p{plugin}"),"mcpServers":entries}).to_string(),
        );
    }
    let result = scan(p);
    assert!(result["items"].as_array().unwrap().len() <= 1000);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == "plugin_budget_exceeded")
            .count(),
        1
    );
    fs::remove_dir_all(p.join("home/.claude/plugins/cache")).unwrap();
    for plugin in 0..130 {
        write(
            p,
            &format!("home/.claude/plugins/cache/m/p{plugin}/1/.claude-plugin/plugin.json"),
            &json!({"name":format!("p{plugin}"),"mcpServers":{"one":{"command":"never"}}})
                .to_string(),
        );
    }
    let result = scan(p);
    assert!(result["items"].as_array().unwrap().len() <= 128);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == "plugin_budget_exceeded")
            .count(),
        1
    );
}
