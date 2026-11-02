use std::process::{Command, Output};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("home")).unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skillrail"));
        command.args(["--home"]).arg(self.root.path().join("home"));
        command
            .args(["--project"])
            .arg(self.root.path().join("project"));
        command
            .args(["--state-dir"])
            .arg(self.root.path().join("state"));
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}

#[test]
fn version_help_and_invalid_arguments_are_executable() {
    let fixture = Fixture::new();
    let version = fixture.run(&["version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "skillrail 0.1.0-dev"
    );
    let help = fixture.run(&["--help"]);
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("--state-dir")
    );
    assert_eq!(fixture.run(&["not-a-command"]).status.code(), Some(2));
    assert!(!fixture.root.path().join("state").exists());
}

fn json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn isolated_empty_inventory_rebuilds_and_explains_partial_coverage() {
    let fixture = Fixture::new();
    // An invalid fixture agent root must produce a safe diagnostic.
    std::fs::write(
        fixture.root.path().join("home/.claude"),
        "not an agent directory",
    )
    .unwrap();
    let mut first = json(fixture.run(&["scan", "--json"]));
    assert_eq!(first["schema_version"], 1);
    assert_eq!(first["items"], serde_json::json!([]));
    assert_eq!(first["occurrences"], serde_json::json!([]));
    assert_eq!(first["discovery"]["status"], "inventory_partial");
    assert_eq!(first["diagnostics"][0]["code"], "coverage_partial");
    for (field, dir) in [
        ("home", "home"),
        ("project", "project"),
        ("state_dir", "state"),
    ] {
        assert_eq!(
            first["context"][field],
            fixture
                .root
                .path()
                .join(dir)
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
    }
    assert!(first["scanned_at_unix_ms"].as_u64().unwrap() > 0);
    let cache = fixture.root.path().join("state/index.sqlite3");
    let connection = rusqlite::Connection::open(&cache).unwrap();
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let stored: String = connection
        .query_row("SELECT inventory_json FROM scan WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        first
    );
    drop(connection);
    std::fs::remove_file(&cache).unwrap();
    let mut second = json(fixture.run(&["list", "--json"]));
    assert!(cache.is_file());
    first.as_object_mut().unwrap().remove("scanned_at_unix_ms");
    second.as_object_mut().unwrap().remove("scanned_at_unix_ms");
    assert_eq!(first, second);
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("home/.claude")).unwrap(),
        "not an agent directory"
    );
}

#[test]
fn invalid_paths_fail_without_stdout_or_partial_state() {
    let fixture = Fixture::new();
    for flag in ["--home", "--project"] {
        let output = fixture
            .command()
            .args(["scan", "--json", flag])
            .arg(fixture.root.path().join("home/.missing-parent/child"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!fixture.root.path().join("state").exists());
    }
    let blocker = fixture.root.path().join("blocker");
    std::fs::write(&blocker, "preserve").unwrap();
    let output = fixture
        .command()
        .args(["list", "--json", "--state-dir"])
        .arg(blocker.join("child"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["error"]["code"], "invalid_state_dir");
    assert_eq!(std::fs::read_to_string(blocker).unwrap(), "preserve");
}

#[test]
fn tui_refuses_piped_io_without_creating_state() {
    let fixture = Fixture::new();
    for args in [vec![], vec!["tui"]] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("terminal_required"));
        assert!(!fixture.root.path().join("state").exists());
    }
}

#[test]
fn incompatible_cache_fails_safely_and_can_be_discarded() {
    let fixture = Fixture::new();
    json(fixture.run(&["scan", "--json"]));
    let cache = fixture.root.path().join("state/index.sqlite3");
    let connection = rusqlite::Connection::open(&cache).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    drop(connection);
    let before = std::fs::read(&cache).unwrap();
    let output = fixture.run(&["list", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "index_version_unsupported");
    assert_eq!(std::fs::read(&cache).unwrap(), before);
    std::fs::remove_file(&cache).unwrap();
    json(fixture.run(&["list", "--json"]));
}

#[cfg(unix)]
#[test]
fn cache_symlink_is_refused_without_writing_target() {
    let fixture = Fixture::new();
    let state = fixture.root.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let target = fixture.root.path().join("preserve");
    std::fs::write(&target, "unchanged").unwrap();
    std::os::unix::fs::symlink(&target, state.join("index.sqlite3")).unwrap();
    let output = fixture.run(&["scan", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "unchanged");
}
