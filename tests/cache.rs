use std::process::{Command, Output};

fn scan(root: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skillrail"))
        .arg("--home")
        .arg(root)
        .arg("--project")
        .arg(root)
        .arg("--state-dir")
        .arg(root.join("state"))
        .args(["scan", "--json"])
        .output()
        .unwrap()
}

#[test]
fn altered_schema_cannot_silently_discard_or_replace_snapshot() {
    for trigger in [
        "CREATE TRIGGER discard BEFORE INSERT ON scan BEGIN SELECT RAISE(IGNORE); END",
        "CREATE TRIGGER discard AFTER INSERT ON scan BEGIN DELETE FROM scan; END",
        "CREATE TRIGGER replace_snapshot AFTER INSERT ON scan BEGIN UPDATE scan SET inventory_json = '{}'; END",
    ] {
        let root = tempfile::tempdir().unwrap();
        assert!(scan(root.path()).status.success());
        let cache = root.path().join("state/index.sqlite3");
        let connection = rusqlite::Connection::open(&cache).unwrap();
        let original: String = connection
            .query_row("SELECT inventory_json FROM scan", [], |r| r.get(0))
            .unwrap();
        connection.execute_batch(trigger).unwrap();
        drop(connection);
        let before = std::fs::read(&cache).unwrap();
        let output = scan(root.path());
        assert_eq!(output.status.code(), Some(1), "{trigger}: {output:?}");
        assert!(output.stdout.is_empty());
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "index_unavailable");
        assert_eq!(std::fs::read(&cache).unwrap(), before);
        let connection = rusqlite::Connection::open(cache).unwrap();
        let saved: String = connection
            .query_row("SELECT inventory_json FROM scan", [], |r| r.get(0))
            .unwrap();
        assert_eq!(saved, original);
    }
}

#[test]
fn negative_schema_version_reports_unsupported_without_changing_cache() {
    let root = tempfile::tempdir().unwrap();
    assert!(scan(root.path()).status.success());
    let cache = root.path().join("state/index.sqlite3");
    let connection = rusqlite::Connection::open(&cache).unwrap();
    connection.pragma_update(None, "user_version", -1).unwrap();
    drop(connection);
    let before = std::fs::read(&cache).unwrap();
    let output = scan(root.path());
    assert_eq!(output.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "index_version_unsupported");
    assert_eq!(std::fs::read(cache).unwrap(), before);
}
