#![cfg(unix)]

use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

fn command(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_skillrail"));
    command.arg("--home").arg(root);
    command.arg("--project").arg(root);
    command.arg("--state-dir").arg(root.join("state"));
    command
}

#[test]
fn closed_stdout_returns_operational_error_without_panicking() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["scan", "--json"],
        vec!["list", "--json"],
        vec!["list"],
        vec!["version"],
        vec!["--version"],
        vec!["--help"],
    ] {
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        let fd: OwnedFd = writer.into();
        let output = command(root.path())
            .args(&args)
            .stdout(Stdio::from(fd))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains("panicked"));
        if args.contains(&"--json") {
            let error: serde_json::Value = serde_json::from_str(&stderr).unwrap();
            assert_eq!(error["error"]["code"], "output_unavailable");
        } else {
            assert!(stderr.starts_with("output_unavailable:"));
        }
    }
}

#[test]
fn human_paths_escape_controls_while_json_preserves_paths() {
    let root = tempfile::tempdir().unwrap();
    let project = root
        .path()
        .join("project-\u{1b}[2J\nforged\u{9}\u{85}\u{202e}");
    std::fs::create_dir(&project).unwrap();
    let plain = command(root.path())
        .arg("list")
        .arg("--project")
        .arg(&project)
        .output()
        .unwrap();
    assert!(plain.status.success());
    let text = String::from_utf8(plain.stdout).unwrap();
    assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
    assert!(!text.contains('\u{202e}'));
    assert_eq!(text.lines().count(), 6, "path newline must not forge a row");
    let json = command(root.path())
        .args(["list", "--json"])
        .arg("--project")
        .arg(&project)
        .output()
        .unwrap();
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        value["context"]["project"],
        project.canonicalize().unwrap().to_str().unwrap()
    );
}
