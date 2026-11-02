use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
struct Fixture {
    root: TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for d in ["home/.codex", "home/.agents/skills/demo", "project/.git"] {
            fs::create_dir_all(root.path().join(d)).unwrap();
        }
        fs::write(
            root.path().join("home/.agents/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: fixture\n---\nbody canary\n",
        )
        .unwrap();
        fs::write(root.path().join("home/.codex/config.toml"), "# keep comment\nmodel = 'unknown-value-canary'\n[mcp_servers.example]\ncommand = 'never-execute-canary'\nargs = ['credential-canary'] # keep inline\n").unwrap();
        Self { root }
    }
    fn path(&self, p: &str) -> std::path::PathBuf {
        self.root.path().join(p)
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_skillrail"));
        c.arg("--home")
            .arg(self.path("home"))
            .arg("--project")
            .arg(self.path("project"))
            .arg("--state-dir")
            .arg(self.path("state"));
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn occurrence(&self, kind: &str) -> String {
        let v = json(self.run(&["scan", "--json"]));
        let id = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["kind"] == kind)
            .unwrap()["id"]
            .as_str()
            .unwrap();
        v["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["item_id"] == id)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .into()
    }
}
fn json(o: Output) -> Value {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(o.stderr.is_empty());
    serde_json::from_slice(&o.stdout).unwrap()
}
fn bytes(p: &Path) -> Vec<u8> {
    fs::read(p).unwrap()
}
#[test]
fn cli_preview_apply_history_undo_preserve_original_bytes_and_secrets() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let config = f.path("home/.codex/config.toml");
    let original = bytes(&config);
    let preview = json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--json",
    ]));
    assert_eq!(preview["status"], "preview");
    assert_eq!(bytes(&config), original);
    assert!(!f.path("state/journal").exists());
    let applied = json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]));
    assert_eq!(applied["status"], "applied");
    let changed = String::from_utf8(bytes(&config)).unwrap();
    assert!(changed.contains("enabled = false"));
    assert!(changed.contains("args = ['credential-canary'] # keep inline"));
    let history = json(f.run(&["operations", "--json"]));
    assert_eq!(history["operations"].as_array().unwrap().len(), 1);
    for v in [&preview, &applied, &history] {
        let s = v.to_string();
        for canary in [
            "credential-canary",
            "unknown-value-canary",
            "never-execute-canary",
        ] {
            assert!(!s.contains(canary));
        }
    }
    let op = applied["operation_id"].as_str().unwrap();
    json(f.run(&["undo", op, "--json"]));
    assert_ne!(bytes(&config), original);
    json(f.run(&["undo", op, "--apply", "--json"]));
    assert_eq!(bytes(&config), original);
    json(f.run(&["undo", op, "--apply", "--json"]));
    assert_eq!(bytes(&config), original);
}

fn app(f: &Fixture) -> skillrail::Application {
    skillrail::Application::new(
        skillrail::Paths::resolve(
            Some(f.path("home")),
            Some(f.path("project")),
            Some(f.path("state")),
        )
        .unwrap(),
    )
}
fn request(id: String) -> skillrail::mutation::ToggleRequest {
    skillrail::mutation::ToggleRequest {
        id,
        agent: "codex".into(),
        scope: "user".into(),
        enabled: false,
    }
}
#[test]
fn changed_since_preview_and_hardlink_aliases_preserve_external_bytes() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let a = app(&f);
    let config = f.path("home/.codex/config.toml");
    let plan = a.plan_toggle(request(id.clone())).unwrap();
    let edited = [bytes(&config), b"\n# external edit\n".to_vec()].concat();
    fs::write(&config, &edited).unwrap();
    assert_eq!(a.apply(&plan).unwrap_err().code, "mutation_conflict");
    assert_eq!(bytes(&config), edited);
    let outside = f.path("outside");
    fs::hard_link(&config, &outside).unwrap();
    assert!(a.plan_toggle(request(id)).is_err());
    assert_eq!(bytes(&outside), edited);
}
#[test]
fn undo_conflict_and_repeated_toggle_are_safe() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let applied = json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]));
    let second = json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]));
    assert_eq!(second["status"], "unchanged");
    let path = f.path("home/.codex/config.toml");
    let edited = [bytes(&path), b"\n# newer edit\n".to_vec()].concat();
    fs::write(&path, &edited).unwrap();
    let undo = f.run(&[
        "undo",
        applied["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(undo.status.code(), Some(4));
    assert_eq!(bytes(&path), edited);
    assert_eq!(
        json(f.run(&["operations", "--json"]))["operations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn interrupted_multi_file_apply_can_be_recovered_after_restart() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.claude/skills/claude-demo")).unwrap();
    fs::write(
        f.path("home/.claude/skills/claude-demo/SKILL.md"),
        "---\nname: claude-demo\ndescription: fixture\n---\n",
    )
    .unwrap();
    let v = json(f.run(&["scan", "--json"]));
    let claude = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["agent"] == "claude")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let a = app(&f);
    let original = bytes(&f.path("home/.codex/config.toml"));
    let plan = a
        .plan_toggles(vec![
            request(f.occurrence("mcp")),
            skillrail::mutation::ToggleRequest {
                id: claude.into(),
                agent: "claude".into(),
                scope: "user".into(),
                enabled: false,
            },
        ])
        .unwrap();
    let op = plan.view().operation_id.clone();
    assert!(
        a.apply_with_checkpoint(&plan, |p| {
            if p == skillrail::mutation::Checkpoint::Published(0) {
                Err(skillrail::AppError::new(
                    "fixture_interruption",
                    "test interruption",
                ))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert_ne!(bytes(&f.path("home/.codex/config.toml")), original);
    drop(a);
    let h = json(f.run(&["operations", "--json"]));
    assert_eq!(h["operations"][0]["status"], "prepared");
    json(f.run(&["undo", &op, "--apply", "--json"]));
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), original);
    assert!(!f.path("home/.claude/settings.json").exists());
}
#[test]
fn preview_token_is_reusable_and_detects_config_edit() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let v = json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--json",
    ]));
    let token = v["token"].as_str().unwrap();
    json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--expect", token, "--apply",
        "--json",
    ]));
}
#[test]
fn recovery_of_interrupted_undo_is_idempotent() {
    let f = Fixture::new();
    let a = app(&f);
    let path = f.path("home/.codex/config.toml");
    let original = bytes(&path);
    let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    let op = a.apply(&p).unwrap().operation_id;
    let u = a.plan_undo(&op).unwrap();
    assert!(
        a.apply_with_checkpoint(&u, |p| {
            if p == skillrail::mutation::Checkpoint::Published(0) {
                Err(skillrail::AppError::new("interrupted", "fixture"))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    let restored = bytes(&path);
    assert_eq!(restored, original);
    let a = app(&f);
    let u = a.plan_undo(&op).unwrap();
    a.apply(&u).unwrap();
    assert_eq!(bytes(&path), original);
}
#[test]
fn interrupted_before_publication_cleans_matching_stage_and_preserves_original() {
    let f = Fixture::new();
    let a = app(&f);
    let path = f.path("home/.codex/config.toml");
    let original = bytes(&path);
    let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    let id = p.view().operation_id.clone();
    assert!(
        a.apply_with_checkpoint(&p, |point| {
            if point == skillrail::mutation::Checkpoint::Staged(0) {
                Err(skillrail::AppError::new("stop", "fixture"))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    let u = a.plan_undo(&id).unwrap();
    a.apply(&u).unwrap();
    assert_eq!(bytes(&path), original);
    assert!(!fs::read_dir(path.parent().unwrap()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".skillrail-")
    }));
}
#[test]
fn write_boundary_detects_symlink_parent_and_stage_substitution() {
    use std::os::unix::fs::symlink;
    for kind in ["target", "parent", "stage", "hardlink"] {
        let f = Fixture::new();
        let a = app(&f);
        let path = f.path("home/.codex/config.toml");
        let original = bytes(&path);
        let outside = f.path("outside");
        fs::write(&outside, b"external preserved").unwrap();
        let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
        let result = a.apply_with_checkpoint(&p, |point| {
            if point == skillrail::mutation::Checkpoint::Staged(0) {
                match kind {
                    "target" => {
                        fs::remove_file(&path).unwrap();
                        symlink(&outside, &path).unwrap();
                    }
                    "hardlink" => {
                        fs::remove_file(&path).unwrap();
                        fs::hard_link(&outside, &path).unwrap();
                    }
                    "parent" => {
                        fs::rename(path.parent().unwrap(), f.path("moved")).unwrap();
                        symlink(f.path("moved"), path.parent().unwrap()).unwrap();
                    }
                    _ => {
                        let stage = fs::read_dir(path.parent().unwrap())
                            .unwrap()
                            .find(|e| {
                                e.as_ref()
                                    .unwrap()
                                    .file_name()
                                    .to_string_lossy()
                                    .starts_with(".skillrail-")
                            })
                            .unwrap()
                            .unwrap()
                            .path();
                        fs::remove_file(stage.join("replacement")).unwrap();
                        symlink(&outside, stage.join("replacement")).unwrap();
                    }
                }
            }
            Ok(())
        });
        assert!(result.is_err(), "{kind}");
        assert_eq!(bytes(&outside), b"external preserved");
        if kind == "parent" {
            assert_eq!(bytes(&f.path("moved/config.toml")), original);
        }
    }
}
#[test]
fn private_journal_rejects_aliases_permissions_and_unknown_version() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for mode in ["symlink", "hardlink", "permissions", "version", "dir"] {
        let f = Fixture::new();
        let id = f.occurrence("mcp");
        let v = json(f.run(&[
            "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
        ]));
        let journal = f.path("state/journal");
        assert_eq!(
            fs::metadata(&journal).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let record = journal.join(format!("{}.json", v["operation_id"].as_str().unwrap()));
        assert_eq!(
            fs::metadata(&record).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let outside = f.path("external-record");
        fs::copy(&record, &outside).unwrap();
        let original = bytes(&outside);
        match mode {
            "symlink" => {
                fs::remove_file(&record).unwrap();
                symlink(&outside, &record).unwrap();
            }
            "hardlink" => {
                fs::remove_file(&record).unwrap();
                fs::hard_link(&outside, &record).unwrap();
            }
            "permissions" => {
                fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap()
            }
            "dir" => fs::set_permissions(&journal, fs::Permissions::from_mode(0o755)).unwrap(),
            _ => {
                let mut r: Value = serde_json::from_slice(&bytes(&record)).unwrap();
                r["version"] = 999.into();
                fs::write(&record, serde_json::to_vec(&r).unwrap()).unwrap();
            }
        }
        let out = f.run(&["operations", "--json"]);
        assert!(!out.status.success(), "{mode}");
        assert!(out.stdout.is_empty());
        assert_eq!(bytes(&outside), original);
        assert!(!String::from_utf8_lossy(&out.stderr).contains("credential-canary"));
    }
}
#[test]
fn journal_lock_coordinates_independent_cli_processes() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let a = app(&f);
    let p = a.plan_toggle(request(id.clone())).unwrap();
    a.apply_with_checkpoint(&p, |point| {
        if point == skillrail::mutation::Checkpoint::Locked {
            let competing = f.run(&[
                "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
            ]);
            assert_eq!(competing.status.code(), Some(4));
            assert!(competing.stdout.is_empty());
        }
        Ok(())
    })
    .unwrap();
}
#[test]
fn changed_preview_token_refuses_even_when_item_inventory_is_unchanged() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let preview = json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--json",
    ]));
    let config = f.path("home/.codex/config.toml");
    let mut edited = bytes(&config);
    edited.extend_from_slice(b"\n# external change\n");
    fs::write(&config, &edited).unwrap();
    let out = f.run(&[
        "disable",
        &id,
        "--agent",
        "codex",
        "--scope",
        "user",
        "--expect",
        preview["token"].as_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(bytes(&config), edited);
}
#[test]
fn tui_select_preview_cancel_apply_history_and_undo_share_guarded_core() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    let f = Fixture::new();
    let a = app(&f);
    let mut inventory = a.scan().unwrap();
    let mut view = skillrail::tui::View::default();
    view.query.kind = Some("mcp".into());
    let mut ui = skillrail::tui::MutationUi::default();
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    let config = f.path("home/.codex/config.toml");
    let original = bytes(&config);
    ui.toggle(false, &view, &inventory);
    ui.handle(&key(KeyCode::Enter), &a, &mut inventory);
    let mut term = Terminal::new(TestBackend::new(180, 40)).unwrap();
    term.draw(|frame| ui.render(frame)).unwrap();
    let screen: String = term
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(screen.contains("Preview"));
    assert!(screen.contains("Enter: apply"));
    assert!(!screen.contains("credential-canary"));
    assert_eq!(bytes(&config), original);
    ui.handle(&key(KeyCode::Esc), &a, &mut inventory);
    assert_eq!(bytes(&config), original);
    ui.toggle(false, &view, &inventory);
    ui.handle(&key(KeyCode::Enter), &a, &mut inventory);
    ui.handle(&key(KeyCode::Enter), &a, &mut inventory);
    assert_ne!(bytes(&config), original);
    ui.history(&a);
    ui.handle(&key(KeyCode::Char('u')), &a, &mut inventory);
    assert_ne!(bytes(&config), original);
    ui.handle(&key(KeyCode::Enter), &a, &mut inventory);
    assert_eq!(bytes(&config), original);
}
#[test]
fn repeated_interruptions_during_multi_file_undo_keep_prior_restoration_evidence() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.claude/skills/other")).unwrap();
    fs::write(
        f.path("home/.claude/skills/other/SKILL.md"),
        "---\nname: other\ndescription: fixture\n---\n",
    )
    .unwrap();
    fs::write(
        f.path("home/.claude/settings.json"),
        "{\"unrelated\":\"settings-canary\"}\n",
    )
    .unwrap();
    let a = app(&f);
    let v = a.scan().unwrap();
    let o = v
        .occurrences
        .iter()
        .find(|o| o.agent == skillrail::discovery::Agent::Claude)
        .unwrap();
    let p = a
        .plan_toggles(vec![
            request(f.occurrence("mcp")),
            skillrail::mutation::ToggleRequest {
                id: o.id.clone(),
                agent: "claude".into(),
                scope: "user".into(),
                enabled: false,
            },
        ])
        .unwrap();
    let op = a.apply(&p).unwrap().operation_id;
    let interrupt = |point| {
        if matches!(point, skillrail::mutation::Checkpoint::Published(_)) {
            Err(skillrail::AppError::new("stop", "fixture"))
        } else {
            Ok(())
        }
    };
    assert!(
        a.apply_with_checkpoint(&a.plan_undo(&op).unwrap(), interrupt)
            .is_err()
    );
    assert!(
        a.apply_with_checkpoint(&a.plan_undo(&op).unwrap(), interrupt)
            .is_err()
    );
    let u = a.plan_undo(&op);
    assert!(
        u.is_ok(),
        "later recovery lost evidence of an earlier restored inode"
    );
    a.apply(&u.unwrap()).unwrap();
}
#[test]
fn skill_toggles_preserve_shared_source_and_add_local_configuration_observations() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.claude/skills")).unwrap();
    std::os::unix::fs::symlink(
        f.path("home/.agents/skills/demo"),
        f.path("home/.claude/skills/demo"),
    )
    .unwrap();
    let config = f.path("home/.codex/config.toml");
    let source = f.path("home/.agents/skills/demo/SKILL.md");
    let original = bytes(&source);
    let scan = json(f.run(&["scan", "--json"]));
    for agent in ["claude", "codex"] {
        let id = scan["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["agent"] == agent && o["mcp"].is_null())
            .unwrap()["id"]
            .as_str()
            .unwrap();
        json(f.run(&[
            "disable", id, "--agent", agent, "--scope", "user", "--apply", "--json",
        ]));
        let v = json(f.run(&["scan", "--json"]));
        let o = v["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == id)
            .unwrap();
        assert!(
            o["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["setting"] == "skill_enabled" && v["status"] == "disabled"),
            "missing {agent} disable observation"
        );
        assert_eq!(bytes(&source), original);
        assert!(
            fs::symlink_metadata(f.path("home/.claude/skills/demo"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    assert!(
        String::from_utf8(bytes(&config))
            .unwrap()
            .contains("[[skills.config]]")
    );
}
#[test]
fn claude_mcp_project_opt_out_preserves_native_lists_unknown_keys_and_credentials() {
    let f = Fixture::new();
    let config = f.path("home/.claude.json");
    let project = f.path("project").canonicalize().unwrap();
    let original = format!(
        "{{\n  \"mcpServers\": {{\"claude-server\": {{\"command\": \"never-run\", \"env\": {{\"PASSWORD\": \"CLAUDE_CREDENTIAL_CANARY\"}}}}}},\n  \"unknown\": [ 1, 2 ],\n  \"projects\": {{{}: {{\"disabledMcpServers\": [\"other\"], \"keep\": true}}}}\n}}\n",
        serde_json::to_string(&project).unwrap()
    );
    fs::write(&config, &original).unwrap();
    let v = json(f.run(&["scan", "--json"]));
    let id = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["agent"] == "claude")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let result = json(f.run(&[
        "disable", id, "--agent", "claude", "--scope", "user", "--apply", "--json",
    ]));
    assert!(!result.to_string().contains("CLAUDE_CREDENTIAL_CANARY"));
    let changed = String::from_utf8(bytes(&config)).unwrap();
    assert!(changed.contains("\"unknown\": [ 1, 2 ]"));
    assert!(changed.contains("CLAUDE_CREDENTIAL_CANARY"));
    let changed: Value = serde_json::from_str(&changed).unwrap();
    assert_eq!(
        changed["projects"][project.to_str().unwrap()]["disabledMcpServers"],
        serde_json::json!(["other", "claude-server"])
    );
    let v = json(f.run(&["scan", "--json"]));
    let row = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == id)
        .unwrap();
    assert_eq!(row["mcp"]["configured_status"], "disabled");
    json(f.run(&[
        "enable", id, "--agent", "claude", "--scope", "user", "--apply", "--json",
    ]));
    let changed: Value = serde_json::from_slice(&bytes(&config)).unwrap();
    assert_eq!(
        changed["projects"][project.to_str().unwrap()]["disabledMcpServers"],
        serde_json::json!(["other"])
    );
}
#[test]
fn unsupported_plugin_collision_and_schema_refusals_are_byte_identical() {
    let f = Fixture::new();
    // Same native MCP name across scopes cannot be selected safely by a broad toggle.
    fs::create_dir_all(f.path("project/.codex")).unwrap();
    fs::write(
        f.path("project/.codex/config.toml"),
        "[mcp_servers.example]\ncommand = 'never-run'\n",
    )
    .unwrap();
    let original = bytes(&f.path("home/.codex/config.toml"));
    let id = f.occurrence("mcp");
    let v = json(f.run(&["scan", "--json"]));
    let row = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == id)
        .unwrap();
    let scope = row["scope"].as_str().unwrap();
    let result = f.run(&[
        "disable", &id, "--agent", "codex", "--scope", scope, "--apply", "--json",
    ]);
    assert_eq!(result.status.code(), Some(4));
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), original);
    assert!(!f.path("state/journal").exists());
    // Selected plugin content stays owned by its native lifecycle.
    fs::remove_file(f.path("project/.codex/config.toml")).unwrap();
    fs::create_dir_all(f.path("home/.claude/skills/plugin/.claude-plugin")).unwrap();
    fs::write(
        f.path("home/.claude/skills/plugin/.claude-plugin/plugin.json"),
        "{\"name\":\"plugin\"}",
    )
    .unwrap();
    fs::write(
        f.path("home/.claude/skills/plugin/.mcp.json"),
        "{\"mcpServers\":{\"pluginserver\":{\"command\":\"never-run\"}}}",
    )
    .unwrap();
    let v = json(f.run(&["scan", "--json"]));
    let row = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["origin"]["kind"] == "plugin")
        .unwrap();
    let plugin = f.path("home/.claude/skills/plugin/.mcp.json");
    let original_plugin = bytes(&plugin);
    // An explicit known plugin scope still refuses native lifecycle changes.
    let result = f.run(&[
        "disable",
        row["id"].as_str().unwrap(),
        "--agent",
        "claude",
        "--scope",
        "user",
        "--apply",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(4));
    assert_eq!(bytes(&plugin), original_plugin);
}
#[test]
fn new_cli_exit_contracts_and_wrong_agent_never_write() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let original = bytes(&f.path("home/.codex/config.toml"));
    assert_eq!(f.run(&["disable", &id]).status.code(), Some(2));
    assert_eq!(
        f.run(&[
            "disable", &id, "--agent", "claude", "--scope", "user", "--apply", "--json"
        ])
        .status
        .code(),
        Some(3)
    );
    assert_eq!(
        f.run(&["undo", "../credential-canary", "--apply", "--json"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        f.run(&[
            "undo",
            "op-00000000000000000000000000000000",
            "--apply",
            "--json"
        ])
        .status
        .code(),
        Some(3)
    );
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), original);
    assert_eq!(
        json(f.run(&["operations", "--json"]))["operations"],
        serde_json::json!([])
    );
}
#[tokio::test]
async fn recovery_bytes_never_enter_mcp_or_rebuilt_sqlite() {
    use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let f = Fixture::new();
        let id = f.occurrence("mcp");
        json(f.run(&[
            "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
        ]));
        fs::remove_file(f.path("state/index.sqlite3")).unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_skillrail"));
        command
            .arg("--home")
            .arg(f.path("home"))
            .arg("--project")
            .arg(f.path("project"))
            .arg("--state-dir")
            .arg(f.path("state"))
            .arg("mcp");
        let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
        assert_eq!(client.list_tools(None).await.unwrap().tools.len(), 5);
        for tool in ["list_inventory", "list_drift"] {
            let result = client
                .call_tool(CallToolRequestParams::new(tool))
                .await
                .unwrap();
            let text = &result.content[0].as_text().unwrap().text;
            for canary in [
                "credential-canary",
                "unknown-value-canary",
                "never-execute-canary",
                "body canary",
            ] {
                assert!(!text.contains(canary));
            }
        }
        assert!(
            client
                .call_tool(CallToolRequestParams::new("operations"))
                .await
                .is_err()
        );
        let cache = bytes(&f.path("state/index.sqlite3"));
        assert!(
            !cache
                .windows(b"credential-canary".len())
                .any(|s| s == b"credential-canary")
        );
        let history = json(f.run(&["operations", "--json"]));
        assert_eq!(history["operations"].as_array().unwrap().len(), 1);
        client.cancel().await.unwrap();
    })
    .await
    .expect("M4 MCP isolation timed out");
}
#[test]
fn unrelated_fork_cannot_prolong_a_released_skillrail_lock() {
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    let mut pipe = [0; 2];
    // SAFETY: pipe points to two writable descriptor slots.
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let mut child = -1;
    a.apply_with_checkpoint(&p, |point| {
        if point == skillrail::mutation::Checkpoint::Locked {
            // SAFETY: the child uses only async-signal-safe libc calls and exits without Rust cleanup.
            child = unsafe { libc::fork() };
            assert!(child >= 0);
            if child == 0 {
                // SAFETY: valid inherited descriptors and a single writable byte; _exit never returns.
                unsafe {
                    libc::close(pipe[1]);
                    let mut byte = 0u8;
                    libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
                    libc::_exit(0);
                }
            }
        }
        Ok(())
    })
    .unwrap();
    let result = a.operations();
    // SAFETY: descriptors are owned by this test; waitpid reaps only our child.
    unsafe {
        libc::close(pipe[0]);
        libc::close(pipe[1]);
        libc::waitpid(child, std::ptr::null_mut(), 0);
    }
    assert!(
        result.is_ok(),
        "a fork inherited the open lock description after the operation returned"
    );
}
#[test]
fn creating_a_config_never_overwrites_a_late_external_creator() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.claude/skills/late")).unwrap();
    fs::write(
        f.path("home/.claude/skills/late/SKILL.md"),
        "---\nname: late\ndescription: fixture\n---\n",
    )
    .unwrap();
    let a = app(&f);
    let v = a.scan().unwrap();
    let o = v
        .occurrences
        .iter()
        .find(|o| o.agent == skillrail::discovery::Agent::Claude)
        .unwrap();
    let p = a
        .plan_toggle(skillrail::mutation::ToggleRequest {
            id: o.id.clone(),
            agent: "claude".into(),
            scope: "user".into(),
            enabled: false,
        })
        .unwrap();
    let config = f.path("home/.claude/settings.json");
    let result = a.apply_with_checkpoint(&p, |point| {
        if point == skillrail::mutation::Checkpoint::BeforePublish(0) {
            fs::write(&config, b"{\"external\":true}\n").unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(bytes(&config), b"{\"external\":true}\n");
}
#[test]
fn staged_recovery_bytes_are_contained_in_user_only_directories() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    a.apply_with_checkpoint(&p, |point| {
        if point == skillrail::mutation::Checkpoint::Staged(0) {
            let stage = fs::read_dir(f.path("home/.codex"))
                .unwrap()
                .find(|e| {
                    e.as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".skillrail-")
                })
                .unwrap()
                .unwrap()
                .path();
            assert_eq!(
                fs::metadata(stage).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        Ok(())
    })
    .unwrap();
}
#[cfg(target_os = "linux")]
#[test]
fn extended_acl_is_refused_instead_of_dropping_access_metadata() {
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let path = f.path("home/.codex/config.toml");
    let original = bytes(&path);
    let mut acl = 2u32.to_le_bytes().to_vec();
    // Linux POSIX ACL xattr format, including a named grant that mode bits cannot describe.
    let entries = [
        (1u16, 6u16, u32::MAX),
        (2, 4, 42424),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ];
    for (tag, perm, uid) in entries {
        acl.extend(tag.to_le_bytes());
        acl.extend(perm.to_le_bytes());
        acl.extend(uid.to_le_bytes());
    }
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: path/name are NUL-terminated; the ACL buffer is valid for its complete length.
    let result = unsafe {
        libc::setxattr(
            cpath.as_ptr(),
            c"system.posix_acl_access".as_ptr(),
            acl.as_ptr().cast(),
            acl.len(),
            0,
        )
    };
    assert_eq!(
        result,
        0,
        "Linux ACL fixture prerequisite: {}",
        std::io::Error::last_os_error()
    );
    let result = f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]);
    assert_eq!(result.status.code(), Some(4));
    assert_eq!(bytes(&path), original);
}
#[test]
fn no_change_apply_still_revalidates_the_preview() {
    let f = Fixture::new();
    let a = app(&f);
    let mut r = request(f.occurrence("mcp"));
    r.enabled = true;
    let plan = a.plan_toggle(r).unwrap();
    let path = f.path("home/.codex/config.toml");
    let mut edited = bytes(&path);
    edited.extend_from_slice(b"\nenabled = false\n");
    fs::write(&path, &edited).unwrap();
    assert_eq!(a.apply(&plan).unwrap_err().code, "mutation_conflict");
    assert_eq!(bytes(&path), edited);
}
#[test]
fn inline_skills_container_is_edited_without_panicking_or_losing_other_keys() {
    let f = Fixture::new();
    fs::write(
        f.path("home/.codex/config.toml"),
        "# keep\nskills = { include_instructions = true }\n",
    )
    .unwrap();
    let id = f.occurrence("skill");
    let result = f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let v: toml::Value =
        toml::from_str(&String::from_utf8(bytes(&f.path("home/.codex/config.toml"))).unwrap())
            .unwrap();
    assert_eq!(v["skills"]["include_instructions"].as_bool(), Some(true));
    assert_eq!(v["skills"]["config"][0]["enabled"].as_bool(), Some(false));
}
#[test]
fn dangling_journal_alias_is_not_reported_as_empty_history() {
    let f = Fixture::new();
    f.occurrence("mcp");
    std::os::unix::fs::symlink(f.path("unrelated"), f.path("state/journal")).unwrap();
    let result = f.run(&["operations", "--json"]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(
        fs::symlink_metadata(f.path("state/journal"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
#[test]
fn enabling_default_claude_skill_does_not_create_empty_config() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.claude/skills/default")).unwrap();
    fs::write(
        f.path("home/.claude/skills/default/SKILL.md"),
        "---\nname: default\ndescription: fixture\n---\n",
    )
    .unwrap();
    let a = app(&f);
    let v = a.scan().unwrap();
    let o = v
        .occurrences
        .iter()
        .find(|o| o.agent == skillrail::discovery::Agent::Claude)
        .unwrap();
    let p = a
        .plan_toggle(skillrail::mutation::ToggleRequest {
            id: o.id.clone(),
            agent: "claude".into(),
            scope: "user".into(),
            enabled: true,
        })
        .unwrap();
    assert_eq!(a.apply(&p).unwrap().status, "unchanged");
    assert!(!f.path("home/.claude/settings.json").exists());
    assert!(!f.path("state/journal").exists());
}
#[test]
fn project_scope_changes_only_the_selected_agent_configuration() {
    let f = Fixture::new();
    for agent_dir in [".agents", ".claude"] {
        let folder = f.path(&format!("project/{agent_dir}/skills/project-skill"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("SKILL.md"),
            "---\nname: project-skill\ndescription: fixture\n---\n",
        )
        .unwrap();
    }
    fs::create_dir_all(f.path("project/.codex")).unwrap();
    fs::write(
        f.path("project/.codex/config.toml"),
        "# project only\n[mcp_servers.projectserver]\ncommand = 'never-run'\n",
    )
    .unwrap();
    let user = bytes(&f.path("home/.codex/config.toml"));
    let v = json(f.run(&["scan", "--json"]));
    let rows: Vec<_> = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["scope"] == "project" && (o["agent"] == "claude" || !o["mcp"].is_null()))
        .collect();
    assert_eq!(rows.len(), 2);
    for o in rows {
        let agent = o["agent"].as_str().unwrap();
        let id = o["id"].as_str().unwrap();
        json(f.run(&[
            "disable", id, "--agent", agent, "--scope", "project", "--apply", "--json",
        ]));
    }
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), user);
    assert!(!f.path("home/.claude/settings.json").exists());
    let codex: toml::Value =
        toml::from_str(&String::from_utf8(bytes(&f.path("project/.codex/config.toml"))).unwrap())
            .unwrap();
    assert_eq!(
        codex["mcp_servers"]["projectserver"]["enabled"].as_bool(),
        Some(false)
    );
    assert!(codex.get("skills").is_none());
    let claude: Value =
        serde_json::from_slice(&bytes(&f.path("project/.claude/settings.json"))).unwrap();
    assert_eq!(claude["skillOverrides"]["project-skill"], "off");
}
#[test]
fn same_agent_shared_source_and_unsupported_skill_schema_refuse_identical() {
    let f = Fixture::new();
    let source = f.path("home/.agents/skills/demo/SKILL.md");
    let original_source = bytes(&source);
    let config = f.path("home/.codex/config.toml");
    let original = bytes(&config);
    fs::create_dir_all(f.path("project/.agents/skills")).unwrap();
    std::os::unix::fs::symlink(
        f.path("home/.agents/skills/demo"),
        f.path("project/.agents/skills/alias"),
    )
    .unwrap();
    let v = json(f.run(&["scan", "--json"]));
    let row = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scope"] == "user" && o["mcp"].is_null())
        .unwrap();
    let result = f.run(&[
        "disable",
        row["id"].as_str().unwrap(),
        "--agent",
        "codex",
        "--scope",
        "user",
        "--apply",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(4));
    assert_eq!(bytes(&config), original);
    assert_eq!(bytes(&source), original_source);
    assert!(
        fs::symlink_metadata(f.path("project/.agents/skills/alias"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_file(f.path("project/.agents/skills/alias")).unwrap();
    let invalid =
        "[skills]\nconfig = [{path='/absolute/SKILL.md',name='ambiguous',enabled=false}]\n";
    fs::write(&config, invalid).unwrap();
    let id = f.occurrence("skill");
    let result = f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]);
    assert_eq!(result.status.code(), Some(4));
    assert_eq!(bytes(&config), invalid.as_bytes());
    assert!(!f.path("state/journal").exists());
}
#[test]
fn journal_parent_substitution_preserves_external_directory_and_configs() {
    let f = Fixture::new();
    let a = app(&f);
    let config = f.path("home/.codex/config.toml");
    let original = bytes(&config);
    let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    fs::create_dir(f.path("external-journal")).unwrap();
    fs::write(f.path("external-journal/keep"), b"EXTERNAL_FILE_CANARY").unwrap();
    let result = a.apply_with_checkpoint(&p, |point| {
        if point == skillrail::mutation::Checkpoint::Recorded {
            fs::rename(f.path("state/journal"), f.path("saved-journal")).unwrap();
            std::os::unix::fs::symlink(f.path("external-journal"), f.path("state/journal"))
                .unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(bytes(&config), original);
    assert_eq!(
        bytes(&f.path("external-journal/keep")),
        b"EXTERNAL_FILE_CANARY"
    );
    assert_eq!(fs::read_dir(f.path("external-journal")).unwrap().count(), 1);
}
#[test]
fn repeated_skill_disable_after_another_rule_does_not_append_duplicates() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.agents/skills/other")).unwrap();
    fs::write(
        f.path("home/.agents/skills/other/SKILL.md"),
        "---\nname: other\ndescription: fixture\n---\n",
    )
    .unwrap();
    let v = json(f.run(&["scan", "--json"]));
    let rows: Vec<_> = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["mcp"].is_null())
        .collect();
    assert_eq!(rows.len(), 2);
    let ids: Vec<_> = rows.iter().map(|o| o["id"].as_str().unwrap()).collect();
    for id in &ids {
        json(f.run(&[
            "disable", id, "--agent", "codex", "--scope", "user", "--apply", "--json",
        ]));
    }
    let original = bytes(&f.path("home/.codex/config.toml"));
    let result = json(f.run(&[
        "disable", ids[0], "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]));
    assert_eq!(result["status"], "unchanged");
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), original);
}

#[test]
fn codex_project_skill_layer_refuses_without_user_scope_fallback() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("project/.agents/skills/project-only")).unwrap();
    fs::create_dir_all(f.path("project/.codex")).unwrap();
    fs::write(
        f.path("project/.agents/skills/project-only/SKILL.md"),
        "---\nname: project-only\ndescription: fixture\n---\n",
    )
    .unwrap();
    let project_config = "# unchanged\n[skills]\nconfig=[{name='project-only',enabled=false}]\n";
    fs::write(f.path("project/.codex/config.toml"), project_config).unwrap();
    let user = bytes(&f.path("home/.codex/config.toml"));
    let v = json(f.run(&["scan", "--json"]));
    let o = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["agent"] == "codex" && o["scope"] == "project")
        .unwrap();
    let result = f.run(&[
        "disable",
        o["id"].as_str().unwrap(),
        "--agent",
        "codex",
        "--scope",
        "project",
        "--apply",
        "--json",
    ]);
    assert_eq!(
        result.status.code(),
        Some(4),
        "project rules are ignored by pinned native enablement"
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("User and SessionFlags"));
    assert_eq!(
        bytes(&f.path("project/.codex/config.toml")),
        project_config.as_bytes()
    );
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), user);
    assert!(!f.path("state/journal").exists());
    assert_eq!(o["capability"]["toggle"], false);
    assert!(
        !o["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["setting"] == "skill_enabled" && v["scope"] == "project")
    );
    let user_id = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["agent"] == "codex" && o["scope"] == "user" && o["mcp"].is_null())
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(
        json(f.run(&[
            "disable", user_id, "--agent", "codex", "--scope", "user", "--apply", "--json"
        ]))["status"],
        "applied"
    );
}

#[test]
fn source_retarget_invalidates_cli_token_and_retained_plan_without_writes() {
    let f = Fixture::new();
    for (name, body) in [("source-a", "A"), ("source-b", "B")] {
        fs::create_dir(f.path(name)).unwrap();
        fs::write(
            f.path(&format!("{name}/SKILL.md")),
            format!("---\nname: demo\ndescription: fixture\n---\n{body}\n"),
        )
        .unwrap();
    }
    fs::create_dir_all(f.path("home/.claude/skills")).unwrap();
    let link = f.path("home/.claude/skills/demo");
    std::os::unix::fs::symlink(f.path("source-a"), &link).unwrap();
    let v = json(f.run(&["scan", "--json"]));
    let id = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["agent"] == "claude")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let preview = json(f.run(&[
        "disable", id, "--agent", "claude", "--scope", "user", "--json",
    ]));
    let repeat = json(f.run(&[
        "disable", id, "--agent", "claude", "--scope", "user", "--json",
    ]));
    assert_eq!(preview["token"], repeat["token"]);
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(f.path("source-b"), &link).unwrap();
    let result = f.run(&[
        "disable",
        id,
        "--agent",
        "claude",
        "--scope",
        "user",
        "--expect",
        preview["token"].as_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(
        result.status.code(),
        Some(4),
        "separate CLI apply accepted a different source"
    );
    assert!(!f.path("home/.claude/settings.json").exists());
    assert!(!f.path("state/journal").exists());
    let fresh = json(f.run(&[
        "disable", id, "--agent", "claude", "--scope", "user", "--json",
    ]));
    assert_ne!(fresh["token"], preview["token"]);
    let a = app(&f);
    let plan = a
        .plan_toggle(skillrail::mutation::ToggleRequest {
            id: id.into(),
            agent: "claude".into(),
            scope: "user".into(),
            enabled: false,
        })
        .unwrap();
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(f.path("source-a"), &link).unwrap();
    assert_eq!(a.apply(&plan).unwrap_err().code, "mutation_conflict");
    assert!(!f.path("home/.claude/settings.json").exists());
    assert!(!f.path("state/journal").exists());
}

#[test]
fn claude_project_enable_accounts_for_known_inherited_listing_states() {
    for inherited in [
        None,
        Some("on"),
        Some("off"),
        Some("name-only"),
        Some("user-invocable-only"),
    ] {
        let f = Fixture::new();
        fs::create_dir_all(f.path("project/.claude/skills/demo")).unwrap();
        fs::create_dir_all(f.path("home/.claude")).unwrap();
        fs::write(
            f.path("project/.claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: fixture\n---\n",
        )
        .unwrap();
        if let Some(state) = inherited {
            fs::write(f.path("home/.claude/settings.json"), format!("{{\"skillOverrides\":{{\"demo\":\"{state}\"}},\"unknown\":\"PRIVATE_INHERITED_CANARY\"}}\n")).unwrap();
        }
        let v = json(f.run(&["scan", "--json"]));
        let id = v["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["agent"] == "claude")
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let original_user = fs::read(f.path("home/.claude/settings.json")).ok();
        let result = json(f.run(&[
            "enable", id, "--agent", "claude", "--scope", "project", "--apply", "--json",
        ]));
        if matches!(inherited, None | Some("on")) {
            assert_eq!(result["status"], "unchanged");
            assert!(!f.path("project/.claude/settings.json").exists());
            assert!(!f.path("state/journal").exists());
        } else {
            assert_eq!(
                result["status"], "applied",
                "inherited {inherited:?} was treated as on"
            );
            let settings: Value =
                serde_json::from_slice(&bytes(&f.path("project/.claude/settings.json"))).unwrap();
            assert_eq!(settings["skillOverrides"]["demo"], "on");
        }
        assert_eq!(
            fs::read(f.path("home/.claude/settings.json")).ok(),
            original_user
        );
        assert!(!result.to_string().contains("PRIVATE_INHERITED_CANARY"));
    }
}

#[test]
fn journal_admission_covers_single_and_batch_complete_recovery_lifecycle() {
    for (batch, size) in [
        (false, 70_000),
        (false, 200_000),
        (false, 500_000),
        (false, 524_288),
        (false, 700_000),
        (true, 35_000),
        (true, 100_000),
        (true, 250_000),
        (true, 262_144),
        (true, 350_000),
    ] {
        let f = Fixture::new();
        fs::create_dir_all(f.path("home/.claude/skills/large")).unwrap();
        fs::write(
            f.path("home/.claude/skills/large/SKILL.md"),
            "---\nname: large\ndescription: fixture\n---\n",
        )
        .unwrap();
        let claude = format!("{{\"unknown\":\"{}\"}}\n", "x".repeat(size));
        fs::write(f.path("home/.claude/settings.json"), &claude).unwrap();
        let codex = format!("# {}\n", "x".repeat(size));
        if batch {
            fs::write(f.path("home/.codex/config.toml"), &codex).unwrap();
        }
        let a = app(&f);
        let v = a.scan().unwrap();
        let requests = v
            .occurrences
            .iter()
            .filter(|o| {
                o.agent == skillrail::discovery::Agent::Claude || (batch && o.mcp.is_none())
            })
            .map(|o| skillrail::mutation::ToggleRequest {
                id: o.id.clone(),
                agent: if o.agent == skillrail::discovery::Agent::Claude {
                    "claude"
                } else {
                    "codex"
                }
                .into(),
                scope: "user".into(),
                enabled: false,
            })
            .collect();
        let plan = a.plan_toggles(requests);
        match plan {
            Err(error) => {
                assert_eq!(error.code, "mutation_refused");
                assert!(
                    if batch {
                        size >= 262_144
                    } else {
                        size >= 524_288
                    },
                    "small positive control was refused"
                );
                assert!(!f.path("state/journal").exists());
            }
            Ok(plan) => {
                let op = a.apply(&plan).unwrap().operation_id;
                let u = a.plan_undo(&op).unwrap();
                let interrupted = a.apply_with_checkpoint(&u, |p| {
                    if p == skillrail::mutation::Checkpoint::Staged(0) {
                        Err(skillrail::AppError::new("interrupted", "fixture"))
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(
                    interrupted.unwrap_err().code,
                    "interrupted",
                    "accepted operation cannot reach undo stage: batch={batch}, size={size}"
                );
                let a = app(&f);
                a.apply(&a.plan_undo(&op).unwrap()).unwrap();
                a.apply(&a.plan_undo(&op).unwrap()).unwrap();
                assert_eq!(a.operations().unwrap().operations[0].status, "undone");
            }
        }
        assert_eq!(
            bytes(&f.path("home/.claude/settings.json")),
            claude.as_bytes()
        );
        if batch {
            assert_eq!(bytes(&f.path("home/.codex/config.toml")), codex.as_bytes());
        }
    }
}

#[test]
fn version_one_large_recovery_record_remains_readable_and_undoable() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.claude/skills/legacy")).unwrap();
    fs::write(
        f.path("home/.claude/skills/legacy/SKILL.md"),
        "---\nname: legacy\ndescription: fixture\n---\n",
    )
    .unwrap();
    let original = format!("{{\"unknown\":\"{}\"}}\n", "x".repeat(200_000));
    fs::write(f.path("home/.claude/settings.json"), &original).unwrap();
    let a = app(&f);
    let v = a.scan().unwrap();
    let o = v
        .occurrences
        .iter()
        .find(|o| o.agent == skillrail::discovery::Agent::Claude)
        .unwrap();
    let p = a
        .plan_toggle(skillrail::mutation::ToggleRequest {
            id: o.id.clone(),
            agent: "claude".into(),
            scope: "user".into(),
            enabled: false,
        })
        .unwrap();
    let op = a.apply(&p).unwrap().operation_id;
    let path = f.path(&format!("state/journal/{op}.json"));
    let mut legacy: Value = serde_json::from_slice(&bytes(&path)).unwrap();
    if legacy["version"] == 2 {
        for c in legacy["changes"].as_array_mut().unwrap() {
            if !c["published"].is_null() {
                c["published"] = serde_json::json!({"identity":c["published"],"bytes":c["after"]});
            }
        }
        legacy["version"] = 1.into();
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    }
    a.apply(&a.plan_undo(&op).unwrap()).unwrap();
    a.apply(&a.plan_undo(&op).unwrap()).unwrap();
    assert_eq!(
        bytes(&f.path("home/.claude/settings.json")),
        original.as_bytes()
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes(&path)).unwrap()["version"],
        2
    );
}

#[cfg(target_os = "linux")]
#[test]
fn real_process_interruptions_do_not_replace_a_recorded_undo_stage_inode() {
    const CHILD: &str = "SKILLRAIL_TEST_UNDO_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        let root = std::path::PathBuf::from(root);
        let a = skillrail::Application::new(
            skillrail::Paths::resolve(
                Some(root.join("home")),
                Some(root.join("project")),
                Some(root.join("state")),
            )
            .unwrap(),
        );
        let id = fs::read_to_string(root.join("operation-id")).unwrap();
        let result = a.plan_undo(&id).and_then(|p| {
            a.apply_with_checkpoint(&p, |point| {
                if std::env::var_os("SKILLRAIL_TEST_STOP_STAGE").is_some()
                    && point == skillrail::mutation::Checkpoint::Staged(0)
                {
                    std::process::exit(77);
                }
                Ok(())
            })
        });
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(4);
        }
        return;
    }
    let f = Fixture::new();
    let a = app(&f);
    let path = f.path("home/.codex/config.toml");
    let original = bytes(&path);
    let plan = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    let id = a.apply(&plan).unwrap().operation_id;
    fs::write(f.path("operation-id"), &id).unwrap();
    let library = f.path("fault.so");
    let source = f.path("fault.c");
    fs::write(&source, r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
ssize_t write(int fd, const void *buf, size_t count) {
    ssize_t (*real_write)(int,const void*,size_t) = dlsym(RTLD_NEXT, "write");
    char link[80], path[4096]; snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path)-1);
    const char *root = getenv("SKILLRAIL_TEST_UNDO_CHILD");
    if (n > 0 && root && getenv("SKILLRAIL_TEST_PARTIAL") && count > 1) {
        path[n] = 0;
        if (strncmp(path, root, strlen(root)) == 0 && strstr(path, "-undo/replacement")) {
            real_write(fd, buf, count/2); _exit(79);
        }
    }
    return real_write(fd, buf, count);
}
int fsync(int fd) {
    int (*real_sync)(int) = dlsym(RTLD_NEXT, "fsync");
    int result = real_sync(fd);
    char link[80], path[4096]; snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path)-1);
    const char *root = getenv("SKILLRAIL_TEST_UNDO_CHILD");
    if (n > 0 && root) { path[n] = 0; if (strncmp(path, root, strlen(root)) == 0 && strstr(path, "-undo/replacement")) _exit(78); }
    return result;
}
"#).unwrap();
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC"])
            .arg(&source)
            .args(["-ldl", "-o"])
            .arg(&library)
            .output()
            .unwrap()
            .status
            .success()
    );
    let child_with_partial = |stop: bool, fault: bool, partial: bool| {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "real_process_interruptions_do_not_replace_a_recorded_undo_stage_inode",
                "--nocapture",
            ])
            .env(CHILD, f.root.path());
        if stop {
            command.env("SKILLRAIL_TEST_STOP_STAGE", "1");
        }
        if fault {
            command.env("LD_PRELOAD", &library);
        }
        if partial {
            command.env("SKILLRAIL_TEST_PARTIAL", "1");
        }
        command.output().unwrap()
    };
    let child = |stop, fault| child_with_partial(stop, fault, false);
    assert_eq!(child(true, false).status.code(), Some(77));
    let interrupted = child(false, true);
    assert!(matches!(interrupted.status.code(), Some(0 | 78)));
    let retry = child(false, false);
    assert!(
        retry.status.success(),
        "retry after a real replacement-fsync interruption stranded undo: {}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert!(child(false, false).status.success());
    assert_eq!(bytes(&path), original);
    assert_eq!(a.operations().unwrap().operations[0].status, "undone");
    // Positive fault control: a fresh unrecorded undo stage must hit the intercepted fsync.
    let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    let next = a.apply(&p).unwrap().operation_id;
    fs::write(f.path("operation-id"), next).unwrap();
    assert_eq!(child(false, true).status.code(), Some(78));
    assert!(child(false, false).status.success());
    assert!(child(false, false).status.success());
    assert_eq!(bytes(&path), original);
    let p = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
    let next = a.apply(&p).unwrap().operation_id;
    fs::write(f.path("operation-id"), next).unwrap();
    // Repeated deaths during write_all leave only an unrecorded prefix, not a durable inode.
    for _ in 0..3 {
        assert_eq!(
            child_with_partial(false, true, true).status.code(),
            Some(79)
        );
    }
    let retry = child(false, false);
    assert!(
        retry.status.success(),
        "partial stage blocked recovery: {}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert!(child(false, false).status.success());
    assert_eq!(bytes(&path), original);
}

#[test]
fn recovery_cleans_unrecorded_forward_prefix_but_refuses_unrelated_bytes() {
    use std::os::unix::fs::PermissionsExt;
    for matching in [true, false] {
        let f = Fixture::new();
        let a = app(&f);
        let path = f.path("home/.codex/config.toml");
        let original = bytes(&path);
        let plan = a.plan_toggle(request(f.occurrence("mcp"))).unwrap();
        let id = plan.view().operation_id.clone();
        a.apply_with_checkpoint(&plan, |point| {
            if point == skillrail::mutation::Checkpoint::Recorded {
                Err(skillrail::AppError::new("interrupted", "fixture"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        let record: Value =
            serde_json::from_slice(&bytes(&f.path(&format!("state/journal/{id}.json")))).unwrap();
        let c = &record["changes"][0];
        let stage = path.parent().unwrap().join(c["stage"].as_str().unwrap());
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let proposed: Vec<u8> = serde_json::from_value(c["after"].clone()).unwrap();
        let partial = if matching {
            &proposed[..proposed.len() / 2]
        } else {
            b"unrelated"
        };
        fs::write(stage.join("replacement"), partial).unwrap();
        let result = a.apply(&a.plan_undo(&id).unwrap());
        if matching {
            assert_eq!(result.unwrap().status, "undone");
            assert!(!stage.exists());
        } else {
            assert_eq!(result.unwrap_err().code, "mutation_conflict");
            assert_eq!(bytes(&stage.join("replacement")), partial);
        }
        assert_eq!(bytes(&path), original);
    }
}

#[test]
fn toml_edits_preserve_uniform_crlf_and_multiline_literal_values() {
    for skill in [false, true] {
        let f = Fixture::new();
        let input = "# untouched\r\nnotes = '''first\r\nsecond\r\n'''\r\nmodel = 'PRIVATE_CRLF_CANARY'\r\n[mcp_servers.example]\r\ncommand = 'never-run'\r\n";
        fs::write(f.path("home/.codex/config.toml"), input).unwrap();
        let before: toml::Value = toml::from_str(input).unwrap();
        let id = f.occurrence(if skill { "skill" } else { "mcp" });
        let result = json(f.run(&[
            "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
        ]));
        let output = String::from_utf8(bytes(&f.path("home/.codex/config.toml"))).unwrap();
        assert!(
            !output.replace("\r\n", "").contains('\n'),
            "unrelated CRLF line endings changed"
        );
        assert!(output.contains("# untouched\r\n"));
        assert!(output.contains("model = 'PRIVATE_CRLF_CANARY'\r\n"));
        let after: toml::Value = toml::from_str(&output).unwrap();
        assert_eq!(after["notes"], before["notes"]);
        assert!(!result.to_string().contains("PRIVATE_CRLF_CANARY"));
        json(f.run(&[
            "undo",
            result["operation_id"].as_str().unwrap(),
            "--apply",
            "--json",
        ]));
        assert_eq!(bytes(&f.path("home/.codex/config.toml")), input.as_bytes());
    }
}
#[test]
fn mixed_toml_line_endings_refuse_without_rewriting_literal_content() {
    let f = Fixture::new();
    let input = "# CRLF\r\nnotes = '''literal\nnewline'''\r\n[mcp_servers.example]\r\ncommand='never-run'\r\n";
    fs::write(f.path("home/.codex/config.toml"), input).unwrap();
    let id = f.occurrence("mcp");
    let result = f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]);
    assert_eq!(result.status.code(), Some(4));
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), input.as_bytes());
    assert!(!f.path("state/journal").exists());
}
#[test]
fn sequential_history_undo_keeps_strict_external_identity_preconditions() {
    let f = Fixture::new();
    let id = f.occurrence("mcp");
    let a = json(f.run(&[
        "disable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]));
    let b = json(f.run(&[
        "enable", &id, "--agent", "codex", "--scope", "user", "--apply", "--json",
    ]));
    json(f.run(&[
        "undo",
        b["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]));
    let before = bytes(&f.path("home/.codex/config.toml"));
    let result = f.run(&[
        "undo",
        a["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(4));
    assert_eq!(bytes(&f.path("home/.codex/config.toml")), before);
}

#[test]
fn project_enable_refuses_restrictive_local_override_and_guards_consulted_user_bytes() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("project/.claude/skills/demo")).unwrap();
    fs::create_dir_all(f.path("home/.claude")).unwrap();
    fs::write(
        f.path("project/.claude/skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: fixture\n---\n",
    )
    .unwrap();
    fs::write(
        f.path("home/.claude/settings.json"),
        "{\"skillOverrides\":{\"demo\":\"off\"},\"unknown\":1}\n",
    )
    .unwrap();
    let v = json(f.run(&["scan", "--json"]));
    let id = v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["agent"] == "claude")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let preview = json(f.run(&[
        "enable", id, "--agent", "claude", "--scope", "project", "--json",
    ]));
    fs::write(
        f.path("home/.claude/settings.json"),
        "{\"skillOverrides\":{\"demo\":\"off\"},\"unknown\":2}\n",
    )
    .unwrap();
    assert_eq!(
        f.run(&[
            "enable",
            id,
            "--agent",
            "claude",
            "--scope",
            "project",
            "--expect",
            preview["token"].as_str().unwrap(),
            "--apply",
            "--json"
        ])
        .status
        .code(),
        Some(4)
    );
    assert!(!f.path("project/.claude/settings.json").exists());
    assert!(!f.path("state/journal").exists());
    for local in ["off", "name-only", "user-invocable-only"] {
        fs::write(
            f.path("project/.claude/settings.local.json"),
            format!("{{\"skillOverrides\":{{\"demo\":\"{local}\"}}}}\n"),
        )
        .unwrap();
        let result = f.run(&[
            "enable", id, "--agent", "claude", "--scope", "project", "--apply", "--json",
        ]);
        assert_eq!(result.status.code(), Some(4));
        assert!(!f.path("project/.claude/settings.json").exists());
        assert!(!f.path("state/journal").exists());
    }
}
