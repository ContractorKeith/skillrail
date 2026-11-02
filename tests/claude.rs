//! Executable Claude compatibility regressions. Git runs only to create disposable fixtures.
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn skill(root: &Path, folder: &str, name: &str) {
    write(
        &root.join(folder).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: Fixture\n---\n"),
    );
}
fn scan(home: &Path, project: &Path, state: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_skillrail"))
        .arg("--home")
        .arg(home)
        .arg("--project")
        .arg(project)
        .arg("--state-dir")
        .arg(state)
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
fn git(root: &Path, args: &[&std::ffi::OsStr]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=ContractorKeith",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn snapshot(roots: &[&Path]) -> std::collections::BTreeMap<PathBuf, (u32, Vec<u8>)> {
    use std::os::unix::fs::PermissionsExt;
    fn visit(path: &Path, result: &mut std::collections::BTreeMap<PathBuf, (u32, Vec<u8>)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let bytes = if metadata.is_file() {
            fs::read(path).unwrap()
        } else if metadata.is_symlink() {
            fs::read_link(path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else {
            vec![]
        };
        result.insert(path.to_owned(), (metadata.permissions().mode(), bytes));
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(&entry.unwrap().path(), result);
            }
        }
    }
    let mut result = std::collections::BTreeMap::new();
    for root in roots {
        visit(root, &mut result);
    }
    result
}

struct Linked {
    _root: tempfile::TempDir,
    home: PathBuf,
    main: PathBuf,
    linked: PathBuf,
    state: PathBuf,
}
impl Linked {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let p = root.path().canonicalize().unwrap();
        let main = p.join("main");
        let home = p.join("home");
        let linked = p.join("linked");
        fs::create_dir(&main).unwrap();
        fs::create_dir(&home).unwrap();
        git(
            &main,
            &[
                "init".as_ref(),
                "-q".as_ref(),
                "--initial-branch=main".as_ref(),
            ],
        );
        git(
            &main,
            &[
                "commit".as_ref(),
                "-q".as_ref(),
                "--allow-empty".as_ref(),
                "-m".as_ref(),
                "fixture".as_ref(),
            ],
        );
        git(
            &main,
            &[
                "worktree".as_ref(),
                "add".as_ref(),
                "--orphan".as_ref(),
                "-b".as_ref(),
                "fixture-orphan".as_ref(),
                linked.as_os_str(),
            ],
        );
        skill(&main, ".claude/skills/main-skill", "main-skill");
        skill(&main, ".agents/skills/codex-main", "codex-main");
        Self {
            _root: root,
            main,
            home,
            linked,
            state: p.join("state"),
        }
    }
    fn scan(&self) -> Value {
        scan(&self.home, &self.linked, &self.state)
    }
}

#[test]
fn linked_worktree_uses_claude_main_checkout_only_when_root_skills_are_missing() {
    let f = Linked::new();
    let before = snapshot(&[&f.home, &f.main, &f.linked]);
    let result = f.scan();
    assert_eq!(result["items"].as_array().unwrap().len(), 1);
    let occurrence = &result["occurrences"][0];
    assert_eq!(occurrence["agent"], "claude");
    assert_eq!(occurrence["scope"], "project");
    assert_eq!(
        occurrence["original_path"],
        f.main.join(".claude/skills/main-skill").to_str().unwrap()
    );
    assert_eq!(
        occurrence["canonical_path"],
        f.main
            .join(".claude/skills/main-skill")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(
        occurrence["visibility"]["reasons"]
            .to_string()
            .contains("main-checkout fallback")
    );
    assert_eq!(occurrence["visibility"]["status"], "unknown");
    assert_eq!(snapshot(&[&f.home, &f.main, &f.linked]), before);
    // Even an empty present directory suppresses fallback, not merely a matching skill.
    fs::create_dir_all(f.linked.join(".claude/skills")).unwrap();
    assert!(f.scan()["items"].as_array().unwrap().is_empty());
    skill(&f.linked, ".claude/skills/local", "local");
    let local = f.scan();
    assert_eq!(local["items"].as_array().unwrap().len(), 1);
    assert_eq!(local["items"][0]["name"], "local");
}

#[test]
fn unsafe_or_unreadable_linked_metadata_reports_uncertainty_without_fallback() {
    let f = Linked::new();
    let gitfile = f.linked.join(".git");
    let original = fs::read(&gitfile).unwrap();
    for malformed in [
        "gitdir: SECRET_CANARY\nextra\n",
        "gitdir: /missing/.git/worktrees/unknown\n",
    ] {
        fs::write(&gitfile, malformed).unwrap();
        let result = f.scan();
        assert!(result["items"].as_array().unwrap().is_empty());
        assert!(
            result["diagnostics"]
                .to_string()
                .contains("worktree_fallback_unknown")
        );
        assert!(!result.to_string().contains("SECRET_CANARY"));
        assert_eq!(fs::read(&gitfile).unwrap(), malformed.as_bytes());
    }
    let alias = f.main.parent().unwrap().join("admin-alias");
    std::os::unix::fs::symlink(&f.main, &alias).unwrap();
    fs::write(
        &gitfile,
        format!(
            "gitdir: {}/../main/.git/worktrees/linked\n",
            alias.display()
        ),
    )
    .unwrap();
    assert!(
        f.scan()["diagnostics"]
            .to_string()
            .contains("worktree_fallback_unknown")
    );
    fs::write(&gitfile, "x".repeat(8193)).unwrap();
    assert!(
        f.scan()["diagnostics"]
            .to_string()
            .contains("worktree_fallback_unknown")
    );
    fs::write(&gitfile, &original).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&gitfile).unwrap().permissions().mode();
        fs::set_permissions(&gitfile, fs::Permissions::from_mode(0o0)).unwrap();
        let denied = fs::read(&gitfile).is_err();
        let result = f.scan();
        fs::set_permissions(&gitfile, fs::Permissions::from_mode(mode)).unwrap();
        if denied {
            assert!(
                result["diagnostics"]
                    .to_string()
                    .contains("worktree_fallback_unknown")
            );
        }
    }
    let admin = f.main.join(".git/worktrees/linked");
    let commondir = fs::read(admin.join("commondir")).unwrap();
    fs::write(admin.join("commondir"), "../../wrong").unwrap();
    assert!(
        f.scan()["diagnostics"]
            .to_string()
            .contains("worktree_fallback_unknown")
    );
    fs::write(admin.join("commondir"), &commondir).unwrap();
    // Final metadata links cannot redirect reads into arbitrary content.
    fs::remove_file(admin.join("commondir")).unwrap();
    std::os::unix::fs::symlink("gitdir", admin.join("commondir")).unwrap();
    assert!(
        f.scan()["diagnostics"]
            .to_string()
            .contains("worktree_fallback_unknown")
    );
    fs::remove_file(admin.join("commondir")).unwrap();
    fs::write(admin.join("commondir"), &commondir).unwrap();
    fs::write(admin.join("gitdir"), f.main.join(".git").to_str().unwrap()).unwrap();
    assert!(
        f.scan()["diagnostics"]
            .to_string()
            .contains("worktree_fallback_unknown")
    );
}

#[test]
fn claude_invocation_alias_precedence_does_not_suppress_distinct_frontmatter_name() {
    let root = tempfile::tempdir().unwrap();
    let p = root.path();
    let home = p.join("home");
    let project = p.join("project");
    fs::create_dir_all(project.join(".git")).unwrap();
    skill(&home, ".claude/skills/deploy", "personal-alias");
    skill(&project, ".claude/skills/deploy", "project-alias");
    let run = || scan(&home, &project, &p.join("state"));
    let first = run();
    let occurrence = first["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scope"] == "project")
        .unwrap();
    let invocations = occurrence["invocations"]
        .as_array()
        .expect("invocation names separate from source name");
    assert!(
        invocations
            .iter()
            .any(|i| i["name"] == "deploy" && i["shadowed_in_default_layout"] == true)
    );
    assert!(
        invocations
            .iter()
            .any(|i| i["name"] == "project-alias" && i["shadowed_in_default_layout"] == false)
    );
    assert!(
        occurrence["visibility"]["reasons"]
            .to_string()
            .contains("/deploy")
    );
    let identities: std::collections::BTreeMap<_, _> = first["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["original_path"].as_str().unwrap().to_owned(),
                (o["item_id"].clone(), o["id"].clone()),
            )
        })
        .collect();
    let item_id = occurrence["item_id"].clone();
    // A personal frontmatter name can overlap a project folder alias as well.
    skill(&home, ".claude/skills/deploy", "project-alias");
    let second = run();
    let occurrence = second["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scope"] == "project")
        .unwrap();
    assert_eq!(occurrence["item_id"], item_id);
    for occurrence in second["occurrences"].as_array().unwrap() {
        assert_eq!(
            identities[occurrence["original_path"].as_str().unwrap()],
            (occurrence["item_id"].clone(), occurrence["id"].clone())
        );
    }
    assert!(
        occurrence["invocations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["shadowed_in_default_layout"] == true)
    );
    skill(&project, ".claude/skills/ship", "deploy");
    let third = run();
    let ship = third["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["original_path"].as_str().unwrap().ends_with("/ship"))
        .unwrap();
    let invocations = ship["invocations"].as_array().unwrap();
    assert!(
        invocations
            .iter()
            .any(|i| i["name"] == "deploy" && i["shadowed_in_default_layout"] == true)
    );
    assert!(
        invocations
            .iter()
            .any(|i| i["name"] == "ship" && i["shadowed_in_default_layout"] == false)
    );
}

#[test]
fn linked_nested_project_and_relative_metadata_keep_agent_specific_discovery() {
    let f = Linked::new();
    fs::write(
        f.linked.join(".git"),
        "gitdir: ../main/.git/worktrees/linked\n",
    )
    .unwrap();
    let nested = f.linked.join("nested");
    skill(&nested, ".claude/skills/nested", "nested");
    skill(&nested, ".agents/skills/codex-nested", "codex-nested");
    let before = snapshot(&[&f.home, &f.main, &f.linked]);
    let result = scan(&f.home, &nested, &f.state);
    assert_eq!(result["items"].as_array().unwrap().len(), 3);
    assert!(
        !result["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["name"] == "codex-main")
    );
    assert_eq!(snapshot(&[&f.home, &f.main, &f.linked]), before);
    // An invalid present root never authorizes main-checkout fallback.
    std::os::unix::fs::symlink("missing", f.linked.join(".claude")).unwrap();
    assert!(
        f.scan()["diagnostics"]
            .to_string()
            .contains("worktree_fallback_unknown")
    );
}
