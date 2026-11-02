use serde_json::Value;
use std::{fs, path::Path, process::Command};

#[test]
fn failed_root_diagnostic_identifies_location_agent_scope_and_category() {
    let root = fixture();
    let p = root.path();
    write(p, "project/.agents/skills", "not a directory");
    let result = scan(p);
    let diagnostic = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "discovery_incomplete")
        .unwrap();
    assert_eq!(
        diagnostic["location"]["path"],
        p.join("project/.agents/skills").to_str().unwrap()
    );
    assert_eq!(diagnostic["location"]["agent"], "codex");
    assert_eq!(diagnostic["location"]["scope"], "project");
    assert_eq!(diagnostic["location"]["category"], "not_directory");
}

#[test]
fn diagnostic_paths_escape_terminal_controls_but_keep_json_values() {
    let path = "/fixture/project-\u{1b}[2J\n/.agents/skills";
    let diagnostic = skillrail::Diagnostic {
        code: "discovery_incomplete".into(),
        message: "Cannot inspect root".into(),
        location: Some(skillrail::DiagnosticLocation {
            path: path.into(),
            agent: skillrail::discovery::Agent::Codex,
            scope: skillrail::discovery::Scope::Project,
            category: "permission_denied",
        }),
    };
    let human = skillrail::presentation::diagnostic(&diagnostic);
    assert!(!human.contains('\u{1b}'));
    assert!(!human.contains('\n'));
    assert!(human.contains("\\u{1b}[2J\\n"));
    assert_eq!(
        serde_json::to_value(&diagnostic).unwrap()["location"]["path"],
        path
    );
}

#[test]
fn mixed_case_or_duplicate_origin_keys_cannot_merge_sources() {
    let root = fixture();
    let p = root.path();
    let config = "[remote \"origin\"]\nurl = https://github.com/example/shared.git\n";
    for base in ["home/.agents", "project/.agents"] {
        skill(p, &format!("{base}/skills/check"), "check");
        write(p, &format!("{base}/.git/config"), config);
    }
    assert_eq!(scan(p)["items"].as_array().unwrap().len(), 1);
    for extra in [
        "URL = https://github.com/example/second.git\n",
        "[REMOTE \"origin\"]\nurl = https://github.com/example/second.git\n",
        "[remote\t\"origin\"]\nuRl = https://github.com/example/second.git\n",
    ] {
        write(p, "home/.agents/.git/config", &format!("{config}{extra}"));
        let result = scan(p);
        assert_eq!(result["items"].as_array().unwrap().len(), 2, "{extra}");
        assert!(
            result["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i["name_collision"] == true)
        );
    }
}

#[test]
fn inaccessible_skill_directory_remains_unknown_instead_of_disappearing() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture();
    let p = root.path();
    for base in ["home/.agents", "project/.agents"] {
        skill(p, &format!("{base}/skills/check"), "check");
        write(
            p,
            &format!("{base}/.git/config"),
            "[remote \"origin\"]\nurl = https://github.com/example/shared.git\n",
        );
    }
    assert_eq!(scan(p)["items"][0]["drift"], "equal");
    let folder = p.join("project/.agents/skills/check");
    let mode = fs::metadata(&folder).unwrap().permissions().mode();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o0)).unwrap();
    let denied = fs::symlink_metadata(folder.join("SKILL.md")).is_err();
    let result = scan(p);
    fs::set_permissions(&folder, fs::Permissions::from_mode(mode)).unwrap();
    if !denied {
        eprintln!("directory permission denial unavailable for this user");
        return;
    }
    assert_eq!(result["occurrences"].as_array().unwrap().len(), 2);
    assert_eq!(result["items"][0]["drift"], "unknown");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "discovery_incomplete")
    );
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn skill(root: &Path, path: &str, name: &str) {
    write(
        root,
        &format!("{path}/SKILL.md"),
        &format!("---\nname: {name}\ndescription: Fixture skill\n---\nNever execute this.\n"),
    );
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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("home")).unwrap();
    fs::create_dir(root.path().join("project")).unwrap();
    fs::create_dir(root.path().join("project/.git")).unwrap();
    fs::create_dir(root.path().join("home/.git")).unwrap();
    root
}

#[test]
fn sources_aliases_collisions_and_full_folder_drift_are_observable() {
    let root = fixture();
    let p = root.path();
    for base in ["home/.claude/skills/check", "project/.agents/skills/check"] {
        skill(p, base, "check");
        write(p, &format!("{base}/scripts/check.sh"), "exit 99\n");
        write(p, &format!("{base}/references/a.md"), "reference\n");
    }
    let separate = scan(p);
    assert_eq!(separate["items"].as_array().unwrap().len(), 2);
    assert!(
        separate["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["name_collision"] == true)
    );
    // Repository identity requires both origin and the same relative subpath.
    for base in ["home/.claude", "project/.agents"] {
        write(
            p,
            &format!("{base}/.git/config"),
            "[remote \"origin\"]\nurl = https://github.com/example/skills.git\n",
        );
    }
    std::os::unix::fs::symlink(
        p.join("home/.claude/skills/check"),
        p.join("home/.claude/skills/alias"),
    )
    .unwrap();
    let equal = scan(p);
    assert_eq!(equal["items"].as_array().unwrap().len(), 1);
    assert_eq!(equal["occurrences"].as_array().unwrap().len(), 3);
    assert_eq!(equal["items"][0]["drift"], "equal");
    write(
        p,
        "project/.agents/skills/check/scripts/check.sh",
        "exit 98\n",
    );
    let changed = scan(p);
    assert_eq!(changed["items"][0]["id"], equal["items"][0]["id"]);
    assert_eq!(changed["items"][0]["drift"], "drifted");
    write(
        p,
        "project/.agents/skills/check/scripts/check.sh",
        "exit 99\n",
    );
    write(
        p,
        "project/.agents/skills/check/references/a.md",
        "changed\n",
    );
    assert_eq!(scan(p)["items"][0]["drift"], "drifted");
    let broken = p.join("project/.agents/skills/check/broken");
    std::os::unix::fs::symlink("absent", &broken).unwrap();
    let incomplete = scan(p);
    assert_eq!(incomplete["items"][0]["drift"], "unknown");
    assert!(
        incomplete
            .to_string()
            .contains("fingerprint_incomplete at broken")
    );
    fs::remove_file(broken).unwrap();
    let mut a = scan(p);
    fs::remove_file(p.join("state/index.sqlite3")).unwrap();
    let mut b = scan(p);
    a.as_object_mut().unwrap().remove("scanned_at_unix_ms");
    b.as_object_mut().unwrap().remove("scanned_at_unix_ms");
    assert_eq!(a, b);
    assert_eq!(
        fs::read_to_string(p.join("home/.claude/skills/check/scripts/check.sh")).unwrap(),
        "exit 99\n"
    );
}

#[test]
fn unsafe_links_bad_metadata_and_config_secrets_remain_unknown() {
    let root = fixture();
    let p = root.path();
    skill(p, "home/.agents/skills/check", "check");
    let folder = p.join("home/.agents/skills/check");
    std::os::unix::fs::symlink("missing", folder.join("broken")).unwrap();
    std::os::unix::fs::symlink("loop", folder.join("loop")).unwrap();
    std::os::unix::fs::symlink(p.join("project"), folder.join("escape")).unwrap();
    std::os::unix::fs::symlink("absent", p.join("home/.agents/skills/dead")).unwrap();
    skill(p, "project/.claude/skills/bad", "bad");
    write(
        p,
        "project/.claude/skills/bad/SKILL.md",
        "---\nname: [invalid\n---\n",
    );
    write(p, "home/.codex/config.toml", "SECRET_CANARY = [invalid");
    write(
        p,
        "home/.agents/.git/config",
        "[remote \"origin\"]\nurl = https://user:SECRET_CANARY@example.com/repo?key=SECRET_CANARY\n",
    );
    let result = scan(p);
    assert!(!result.to_string().contains("SECRET_CANARY"));
    assert!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["visibility"]["status"] == "unknown")
    );
    assert!(
        result["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["fingerprint"]["sha256"].is_null())
    );
    let diagnostics = result.to_string();
    assert!(diagnostics.contains("metadata_invalid"));
    assert!(diagnostics.contains("config_unknown"));
    assert!(diagnostics.contains("fingerprint_incomplete"));
    let cache = fs::read(p.join("state/index.sqlite3")).unwrap();
    assert!(!cache.windows(13).any(|x| x == b"SECRET_CANARY"));
}

#[test]
fn ancestor_boundary_shadowing_and_trust_uncertainty_are_explained() {
    let root = fixture();
    let p = root.path();
    fs::create_dir_all(p.join("project/.git")).unwrap();
    fs::create_dir_all(p.join("project/nested")).unwrap();
    skill(p, "project/.claude/skills/check", "check");
    skill(p, "project/nested/.claude/skills/other", "other");
    skill(p, "home/.claude/skills/check", "check");
    skill(p, ".claude/skills/outside", "outside");
    skill(p, "project/.agents/skills/codex", "codex");
    write(
        p,
        "home/.codex/config.toml",
        "[[skills.config]]\nname = 'codex'\nenabled = false\n",
    );
    let launch = || {
        let output = Command::new(env!("CARGO_BIN_EXE_skillrail"))
            .arg("--home")
            .arg(p.join("home"))
            .arg("--project")
            .arg(p.join("project/nested"))
            .arg("--state-dir")
            .arg(p.join("state"))
            .args(["list", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let result = launch();
    assert_eq!(result["items"].as_array().unwrap().len(), 4);
    assert!(
        !result["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["name"] == "outside")
    );
    assert!(result.to_string().contains("shadowed_in_default_layout"));
    assert!(result.to_string().contains("last matching rule disables"));
    write(
        p,
        "home/.codex/config.toml",
        "[[skills.config]]\nname = 'codex'\nenabled = false\n[[skills.config]]\nname = 'codex'\nenabled = true\n",
    );
    assert!(!launch().to_string().contains("last matching rule disables"));
    write(
        p,
        "project/.codex/config.toml",
        "[projects]\nsecret = 'SECRET_CANARY'\n",
    );
    let uncertain = launch();
    assert!(
        uncertain
            .to_string()
            .contains("project trust/config unresolved")
    );
    assert!(!uncertain.to_string().contains("SECRET_CANARY"));
    assert!(
        uncertain["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["visibility"]["status"] == "unknown")
    );
}

#[test]
fn metadata_limits_and_unsupported_layouts_do_not_hide_other_skills() {
    let root = fixture();
    let p = root.path();
    skill(p, "home/.agents/skills/valid", "valid");
    skill(p, "home/.agents/skills/large", "large");
    write(p, "home/.agents/skills/large/SKILL.md", &"x".repeat(65537));
    skill(p, "home/.claude/skills/plugin", "plugin");
    write(
        p,
        "home/.claude/skills/plugin/.claude-plugin/plugin.json",
        "{\"name\":\"plugin\"}",
    );
    skill(p, "home/.claude/skills/synced", "synced");
    skill(p, "home/.codex/skills/legacy", "legacy");
    write(
        p,
        "home/.agents/skills/valid/.git/config",
        "[remote \"origin\"]\nurl = ssh://user:SECRET_CANARY@host/repo\n",
    );
    let result = scan(p);
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    let large = result["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "large")
        .unwrap();
    assert!(
        large["diagnostics"]
            .to_string()
            .contains("metadata_invalid")
    );
    assert!(result.to_string().contains("plugin_deferred"));
    assert!(result.to_string().contains("synced_deferred"));
    assert!(!result.to_string().contains("SECRET_CANARY"));
    let valid = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "valid")
        .unwrap();
    assert!(valid["source"]["repository"].is_null());
}

#[test]
fn meaningful_metadata_internal_links_and_exclusions_have_deterministic_hashes() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = fixture();
    let p = root.path();
    skill(p, "home/.agents/skills/check", "check");
    let folder = p.join("home/.agents/skills/check");
    write(p, "home/.agents/skills/check/script", "exit 99");
    symlink("script", folder.join("link")).unwrap();
    let digest = || scan(p)["occurrences"][0]["fingerprint"]["sha256"].clone();
    let original = digest();
    assert!(original.is_string());
    write(p, "home/.agents/skills/check/.git/ignored", "excluded");
    write(
        p,
        "home/.agents/skills/check/.skillrail/ignored",
        "excluded",
    );
    assert_eq!(original, digest());
    let script = folder.join("script");
    let mode = fs::metadata(&script).unwrap().permissions().mode();
    fs::set_permissions(&script, fs::Permissions::from_mode(mode ^ 0o100)).unwrap();
    assert_ne!(original, digest());
    fs::set_permissions(&script, fs::Permissions::from_mode(mode)).unwrap();
    assert_eq!(original, digest());
    fs::create_dir(folder.join("empty")).unwrap();
    assert_ne!(original, digest());
    fs::remove_dir(folder.join("empty")).unwrap();
    fs::remove_file(folder.join("link")).unwrap();
    symlink("SKILL.md", folder.join("link")).unwrap();
    assert_ne!(original, digest());
}

#[test]
fn scans_preserve_fixture_tree_bytes_links_and_modes() {
    use std::collections::BTreeMap;
    use std::os::unix::fs::MetadataExt;
    fn tree(path: &Path, result: &mut BTreeMap<std::path::PathBuf, (u32, Vec<u8>)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let bytes = if metadata.is_symlink() {
            fs::read_link(path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else if metadata.is_file() {
            fs::read(path).unwrap()
        } else {
            vec![]
        };
        result.insert(path.to_path_buf(), (metadata.mode(), bytes));
        if metadata.is_dir() {
            for child in fs::read_dir(path).unwrap() {
                tree(&child.unwrap().path(), result);
            }
        }
    }
    let root = fixture();
    let p = root.path();
    skill(p, "home/.claude/skills/check", "check");
    write(
        p,
        "home/.claude/settings.json",
        "{\"secret\":\"SECRET_CANARY\"}",
    );
    std::os::unix::fs::symlink(
        p.join("home/.claude/skills/check"),
        p.join("home/.claude/skills/alias"),
    )
    .unwrap();
    let snapshot = || {
        let mut result = BTreeMap::new();
        tree(&p.join("home"), &mut result);
        tree(&p.join("project"), &mut result);
        result
    };
    let before = snapshot();
    scan(p);
    scan(p);
    assert_eq!(snapshot(), before);
}

#[test]
fn unreadable_files_and_special_files_never_claim_a_complete_fingerprint() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    let root = fixture();
    let p = root.path();
    skill(p, "home/.agents/skills/check", "check");
    let socket = p.join("home/.agents/skills/check/socket");
    let listener = UnixListener::bind(&socket).unwrap();
    assert!(scan(p)["occurrences"][0]["fingerprint"]["sha256"].is_null());
    drop(listener);
    fs::remove_file(socket).unwrap();
    let file = p.join("home/.agents/skills/check/private");
    fs::write(&file, "inaccessible").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o0)).unwrap();
    if fs::read(&file).is_err() {
        assert!(scan(p)["occurrences"][0]["fingerprint"]["sha256"].is_null());
    } else {
        eprintln!("permission denial unavailable for this user; socket case still exercised");
    }
    fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn provenance_requires_safe_unambiguous_repository_and_subpath_evidence() {
    let root = fixture();
    let p = root.path();
    skill(p, "home/.agents/skills/first", "same");
    skill(p, "project/.agents/skills/second", "same");
    for base in ["home/.agents", "project/.agents"] {
        write(
            p,
            &format!("{base}/.git/config"),
            "[remote \"origin\"]\nurl = https://github.com/example/repo.git\n",
        );
    }
    let different_subpaths = scan(p);
    assert_eq!(different_subpaths["items"].as_array().unwrap().len(), 2);
    assert!(
        different_subpaths["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["name_collision"] == true)
    );
    for url in [
        "https://user:SECRET_CANARY@github.com/example/repo",
        "https://github.com/example/repo?token=SECRET_CANARY",
        "https://github.com/example/repo#SECRET_CANARY",
    ] {
        write(
            p,
            "home/.agents/.git/config",
            &format!("[remote \"origin\"]\nurl = {url}\n"),
        );
        let result = scan(p);
        assert!(!result.to_string().contains("SECRET_CANARY"));
        let first = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["source"]["repository"].is_null())
            .unwrap();
        assert!(
            first["source"]["evidence"]
                .as_str()
                .unwrap()
                .contains("unsafe")
        );
    }
    write(
        p,
        "home/.agents/.git/config",
        "[remote \"origin\"]\nurl = https://github.com/example/repo.git\n[include]\npath = SECRET_CANARY\n",
    );
    let result = scan(p);
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["source"]["repository"].is_null())
    );
    assert!(!result.to_string().contains("SECRET_CANARY"));
}
