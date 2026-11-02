use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
struct Fixture {
    root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            root: tempfile::tempdir().unwrap(),
        };
        for p in ["home", "project/.git", "source"] {
            fs::create_dir_all(f.path(p)).unwrap();
        }
        fs::write(
            f.path("source/SKILL.md"),
            "---\nname: demo\ndescription: fixture\n---\nBODY_CANARY\n",
        )
        .unwrap();
        f
    }
    fn path(&self, p: &str) -> PathBuf {
        self.root.path().canonicalize().unwrap().join(p)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_skillrail"))
            .arg("--home")
            .arg(self.path("home"))
            .arg("--project")
            .arg(self.path("project"))
            .arg("--state-dir")
            .arg(self.path("state"))
            .args(args)
            .output()
            .unwrap()
    }
}
fn json(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn local_snapshot_preview_apply_immutability_inventory_and_undo() {
    let f = Fixture::new();
    let source = f.path("source");
    let args = [
        "install",
        source.to_str().unwrap(),
        "--agent",
        "codex",
        "--scope",
        "user",
        "--json",
    ];
    let preview = json(f.run(&args));
    assert_eq!(preview["status"], "preview");
    assert!(!f.path("home/.agents").exists());
    assert!(!f.path("home/.skillrail-sources").exists());
    assert!(!f.path("state/journal").exists());
    let mut apply = args.to_vec();
    apply.extend(["--apply", "--expect", preview["token"].as_str().unwrap()]);
    let applied = json(f.run(&apply));
    let link = f.path("home/.agents/skills/demo");
    let snapshot = fs::read_link(&link).unwrap();
    assert_ne!(snapshot, source);
    let original = fs::read(snapshot.join("SKILL.md")).unwrap();
    fs::write(source.join("SKILL.md"), "changed").unwrap();
    assert_eq!(fs::read(link.join("SKILL.md")).unwrap(), original);
    assert!(!f.path("home/.claude").exists());
    let inventory = json(f.run(&["scan", "--json"]));
    assert_eq!(inventory["items"][0]["source"]["owner"], "skillrail");
    assert!(!inventory.to_string().contains("BODY_CANARY"));
    let op = applied["operation_id"].as_str().unwrap();
    json(f.run(&["undo", op, "--apply", "--json"]));
    json(f.run(&["undo", op, "--apply", "--json"]));
    assert!(!link.exists());
    assert!(!snapshot.exists());
}
fn git(f: &Fixture, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(f.path("source"))
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
#[test]
fn local_git_exact_revision_selection_and_hostile_config_are_inert() {
    let f = Fixture::new();
    git(&f, &["init", "--template="]);
    git(
        &f,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "add",
            "SKILL.md",
        ],
    );
    git(
        &f,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-m",
            "fixture",
        ],
    );
    let canary = f.path("executed");
    // These are executable hostile inputs; the install must never invoke them.
    fs::write(f.path("source/.gitattributes"), "SKILL.md filter=evil\n").unwrap();
    git(&f, &["add", ".gitattributes"]);
    git(
        &f,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-m",
            "attributes",
        ],
    );
    let rev = git(&f, &["rev-parse", "HEAD"]);
    git(
        &f,
        &[
            "config",
            "core.fsmonitor",
            &format!("touch {}", canary.display()),
        ],
    );
    git(
        &f,
        &[
            "config",
            "filter.evil.smudge",
            &format!("touch {}", canary.display()),
        ],
    );
    git(
        &f,
        &[
            "config",
            "credential.helper",
            &format!("!touch {}", canary.display()),
        ],
    );
    fs::create_dir_all(f.path("source/.git/hooks")).unwrap();
    fs::write(
        f.path("source/.git/hooks/post-checkout"),
        format!("#!/bin/sh\ntouch {}\n", canary.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        f.path("source/.git/hooks/post-checkout"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let include = f.path("hostile-include");
    fs::write(&include, "[extensions]\n    unsupported = true\n").unwrap();
    git(&f, &["config", "include.path", include.to_str().unwrap()]);
    fs::write(f.path("source/SKILL.md"), "uncommitted").unwrap();
    let source = f.path("source");
    let out = json(f.run(&[
        "install",
        source.to_str().unwrap(),
        "--git",
        "--ref",
        &rev,
        "--agent",
        "claude",
        "--scope",
        "project",
        "--apply",
        "--json",
    ]));
    assert_eq!(out["install"]["provenance"]["revision"], rev);
    assert!(
        fs::read_to_string(f.path("project/.claude/skills/demo/SKILL.md"))
            .unwrap()
            .contains("BODY_CANARY")
    );
    assert!(!canary.exists());
    json(f.run(&[
        "undo",
        out["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]));
}
#[test]
fn explicit_selection_and_repeated_agents_link_only_selected_snapshot() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("source/other")).unwrap();
    fs::write(
        f.path("source/other/SKILL.md"),
        "---\nname: other\ndescription: another\n---\n",
    )
    .unwrap();
    let p = f.path("source");
    let p = p.to_str().unwrap();
    assert_eq!(
        f.run(&[
            "install", p, "--agent", "claude", "--scope", "user", "--json"
        ])
        .status
        .code(),
        Some(4)
    );
    let out = json(f.run(&[
        "install", p, "--skill", "other", "--agent", "claude", "--agent", "codex", "--scope",
        "user", "--apply", "--json",
    ]));
    assert_eq!(
        fs::read_link(f.path("home/.claude/skills/other")).unwrap(),
        fs::read_link(f.path("home/.agents/skills/other")).unwrap()
    );
    assert!(!f.path("home/.agents/skills/demo").exists());
    json(f.run(&[
        "undo",
        out["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]));
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
fn request(f: &Fixture) -> skillrail::mutation::InstallRequest {
    skillrail::mutation::InstallRequest {
        source: f.path("source").to_str().unwrap().into(),
        git: false,
        revision: None,
        skill: None,
        agents: vec!["claude".into(), "codex".into()],
        scope: "user".into(),
    }
}
#[test]
fn retained_plan_and_cli_token_refuse_source_edits() {
    let f = Fixture::new();
    let a = app(&f);
    let r = request(&f);
    let p = a.plan_install(r.clone()).unwrap();
    let token = p.view().token.clone();
    assert_eq!(a.plan_install(r).unwrap().view().token, token);
    fs::write(f.path("source/extra"), "new content").unwrap();
    assert_eq!(a.apply(&p).unwrap_err().code, "mutation_conflict");
    assert!(!f.path("state/journal").exists());
    assert_eq!(
        f.run(&[
            "install",
            f.path("source").to_str().unwrap(),
            "--agent",
            "claude",
            "--agent",
            "codex",
            "--scope",
            "user",
            "--expect",
            &token,
            "--apply",
            "--json"
        ])
        .status
        .code(),
        Some(4)
    );
}
#[test]
fn interrupted_tree_and_link_publication_and_repeated_interrupted_undo_recover() {
    use skillrail::mutation::Checkpoint as C;
    for stop in [
        C::Recorded,
        C::TreeFile(0),
        C::Staged(0),
        C::Published(0),
        C::Staged(1),
        C::Published(1),
        C::Published(2),
    ] {
        let f = Fixture::new();
        let a = app(&f);
        let p = a.plan_install(request(&f)).unwrap();
        let op = p.view().operation_id.clone();
        let snapshot = p.view().files[0].clone();
        let err = a
            .apply_with_checkpoint(&p, |c| {
                if c == stop {
                    Err(skillrail::AppError::new("stop", "fixture"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(err.code, "stop", "{stop:?}");
        for _ in 0..3 {
            let u = a.plan_undo(&op).unwrap();
            let _ = a.apply_with_checkpoint(&u, |c| {
                if matches!(c, C::Published(_) | C::TreeFile(_)) {
                    Err(skillrail::AppError::new("stop", "fixture"))
                } else {
                    Ok(())
                }
            });
        }
        let u = a.plan_undo(&op).unwrap();
        a.apply(&u).unwrap();
        a.apply(&a.plan_undo(&op).unwrap()).unwrap();
        assert!(!snapshot.exists(), "{stop:?}");
        assert!(!f.path("home/.agents/skills/demo").exists());
    }
}
#[test]
fn existing_content_links_case_aliases_and_unsafe_sources_refuse_unchanged() {
    use std::os::unix::fs::symlink;
    for kind in [
        "file",
        "dir",
        "link",
        "case",
        "escape",
        "internal",
        "hardlink",
        "fifo",
        "unicode",
        "traversal",
    ] {
        let f = Fixture::new();
        fs::create_dir_all(f.path("home/.agents/skills")).unwrap();
        let outside = f.path("outside");
        fs::write(&outside, "preserve").unwrap();
        match kind {
            "file" => fs::write(f.path("home/.agents/skills/demo"), "preserve").unwrap(),
            "dir" => fs::create_dir(f.path("home/.agents/skills/demo")).unwrap(),
            "link" => symlink(&outside, f.path("home/.agents/skills/demo")).unwrap(),
            "case" => fs::create_dir(f.path("home/.agents/skills/DEMO")).unwrap(),
            "escape" => symlink(&outside, f.path("source/escape")).unwrap(),
            "internal" => symlink("SKILL.md", f.path("source/internal")).unwrap(),
            "hardlink" => fs::hard_link(&outside, f.path("source/alias")).unwrap(),
            "fifo" => {
                use std::os::unix::ffi::OsStrExt;
                let p =
                    std::ffi::CString::new(f.path("source/fifo").as_os_str().as_bytes()).unwrap();
                unsafe {
                    assert_eq!(libc::mkfifo(p.as_ptr(), 0o600), 0);
                }
            }
            "unicode" => fs::write(f.path("source/é"), "unsupported").unwrap(),
            _ => {}
        }
        let a = app(&f);
        let mut r = request(&f);
        if kind == "traversal" {
            r.skill = Some("../source".into());
        }
        assert!(a.plan_install(r).is_err(), "{kind}");
        assert_eq!(fs::read(&outside).unwrap(), b"preserve");
        assert!(!f.path("state/journal").exists());
    }
}
#[test]
fn credential_urls_and_unsafe_transports_never_reach_public_errors_or_cache() {
    let f = Fixture::new();
    for source in [
        "https://user:SECRET_CANARY@example.test/repo",
        "https://example.test/repo?token=SECRET_CANARY",
        "ext::touch SECRET_CANARY",
        "ssh://SECRET_CANARY@example.test/repo",
        "file:///SECRET_CANARY",
    ] {
        let o = f.run(&[
            "install", source, "--git", "--agent", "codex", "--scope", "user", "--apply", "--json",
        ]);
        assert!(!o.status.success());
        assert!(o.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&o.stderr).contains("SECRET_CANARY"));
    }
    let inventory = json(f.run(&["scan", "--json"]));
    assert!(inventory["items"].as_array().unwrap().is_empty());
    assert!(
        !fs::read(f.path("state/index.sqlite3"))
            .unwrap()
            .windows(13)
            .any(|w| w == b"SECRET_CANARY")
    );
}
#[test]
fn undo_preserves_modified_snapshot_unknown_files_links_and_consumers() {
    use std::os::unix::fs::symlink;
    for kind in ["edit", "extra", "link", "consumer", "manifest"] {
        let f = Fixture::new();
        let a = app(&f);
        let p = a.plan_install(request(&f)).unwrap();
        let op = a.apply(&p).unwrap().operation_id;
        let snapshot = p.view().files[0].clone();
        match kind {
            "edit" => fs::write(snapshot.join("SKILL.md"), "external").unwrap(),
            "extra" => fs::write(snapshot.join("unknown"), "external").unwrap(),
            "link" => {
                fs::remove_file(f.path("home/.agents/skills/demo")).unwrap();
                symlink(f.path("source"), f.path("home/.agents/skills/demo")).unwrap();
            }
            "consumer" => symlink(&snapshot, f.path("home/.agents/skills/alias")).unwrap(),
            _ => fs::write(snapshot.join(".skillrail/manifest.json"), "{}").unwrap(),
        }
        assert!(a.plan_undo(&op).is_err(), "{kind}");
        assert!(snapshot.exists());
        assert!(f.path("home/.claude/skills/demo").exists());
    }
}
#[test]
fn recovery_handles_link_created_before_identity_record_and_partial_tree_write() {
    use skillrail::mutation::Checkpoint as C;
    use std::os::unix::fs::{PermissionsExt, symlink};
    for kind in ["link", "prefix", "unknown"] {
        let f = Fixture::new();
        let a = app(&f);
        let p = a.plan_install(request(&f)).unwrap();
        let op = p.view().operation_id.clone();
        let stop = if kind == "link" {
            C::Published(0)
        } else {
            C::Recorded
        };
        a.apply_with_checkpoint(&p, |c| {
            if c == stop {
                Err(skillrail::AppError::new("stop", "fixture"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        let record: Value =
            serde_json::from_slice(&fs::read(f.path(&format!("state/journal/{op}.json"))).unwrap())
                .unwrap();
        let i = &record["install"];
        let stage = i["stage"].as_str().unwrap();
        if kind == "link" {
            let parent = f.path("home/.claude/skills");
            fs::create_dir_all(parent.join(stage)).unwrap();
            fs::set_permissions(parent.join(stage), fs::Permissions::from_mode(0o700)).unwrap();
            symlink(&p.view().files[0], parent.join(stage).join("link")).unwrap();
        } else {
            let store = f.path("home/.skillrail-sources");
            fs::create_dir_all(store.join(stage)).unwrap();
            fs::set_permissions(store.join(stage), fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(
                store.join(stage).join("SKILL.md"),
                if kind == "prefix" {
                    b"---\nname:".as_slice()
                } else {
                    b"unknown".as_slice()
                },
            )
            .unwrap();
        }
        let result = a.plan_undo(&op).and_then(|u| a.apply(&u));
        if kind == "unknown" {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().status, "undone", "{kind}");
        }
    }
}
#[test]
fn snapshot_preserves_empty_directories_binary_assets_and_script_mode() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    fs::create_dir_all(f.path("source/empty/nested")).unwrap();
    fs::write(f.path("source/script"), b"#!/bin/sh\nexit 23\n").unwrap();
    fs::set_permissions(f.path("source/script"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(f.path("source/asset"), [0, 255, 0, 13]).unwrap();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    let op = a.apply(&p).unwrap().operation_id;
    let snapshot = &p.view().files[0];
    assert!(snapshot.join("empty/nested").is_dir());
    assert_eq!(fs::read(snapshot.join("asset")).unwrap(), [0, 255, 0, 13]);
    assert_ne!(
        fs::metadata(snapshot.join("script"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    a.apply(&a.plan_undo(&op).unwrap()).unwrap();
}
#[test]
fn last_publication_boundary_refuses_parent_and_stage_substitutions() {
    use skillrail::mutation::Checkpoint as C;
    use std::os::unix::fs::symlink;
    for kind in ["tree", "parent", "stage", "late-destination"] {
        let f = Fixture::new();
        let a = app(&f);
        let p = a.plan_install(request(&f)).unwrap();
        let op = p.view().operation_id.clone();
        let stop = if kind == "tree" {
            C::BeforePublish(0)
        } else {
            C::BeforePublish(1)
        };
        let result = a.apply_with_checkpoint(&p, |c| {
            if c == stop {
                let record: Value = serde_json::from_slice(
                    &fs::read(f.path(&format!("state/journal/{op}.json"))).unwrap(),
                )
                .unwrap();
                let stage = record["install"]["stage"].as_str().unwrap();
                match kind {
                    "tree" => {
                        let path = f.path("home/.skillrail-sources").join(stage);
                        fs::rename(&path, f.path("moved")).unwrap();
                        symlink(f.path("moved"), &path).unwrap();
                    }
                    "parent" => {
                        let path = f.path("home/.claude/skills");
                        fs::rename(&path, f.path("moved")).unwrap();
                        symlink(f.path("moved"), &path).unwrap();
                    }
                    "stage" => {
                        let path = f.path("home/.claude/skills").join(stage);
                        fs::rename(&path, f.path("moved")).unwrap();
                        fs::create_dir(&path).unwrap();
                    }
                    _ => {
                        fs::write(f.path("home/.claude/skills/demo"), "external").unwrap();
                    }
                }
            }
            Ok(())
        });
        assert!(result.is_err(), "{kind} substitution was accepted");
        if kind == "late-destination" {
            assert_eq!(
                fs::read(f.path("home/.claude/skills/demo")).unwrap(),
                b"external"
            );
        }
        if kind == "stage" {
            assert!(
                f.path("moved/link").is_symlink(),
                "a substituted stage published from an unbound directory"
            );
        }
    }
}
#[test]
fn native_name_alias_collision_refuses_without_reclassifying_external_owner() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("home/.agents/skills/alias")).unwrap();
    fs::copy(
        f.path("source/SKILL.md"),
        f.path("home/.agents/skills/alias/SKILL.md"),
    )
    .unwrap();
    let a = app(&f);
    assert!(a.plan_install(request(&f)).is_err());
    assert!(!f.path("home/.skillrail-sources").exists());
    let inventory = a.scan().unwrap();
    assert_ne!(inventory.items[0].source.owner, "skillrail");
}
#[test]
fn admitted_maximum_content_remains_recoverable_and_over_budget_refuses_before_record() {
    use skillrail::mutation::Checkpoint as C;
    for size in [500_000, 530_000] {
        let f = Fixture::new();
        fs::write(f.path("source/large"), vec![255u8; size]).unwrap();
        let a = app(&f);
        match a.plan_install(request(&f)) {
            Ok(p) => {
                assert_eq!(size, 500_000);
                let op = p.view().operation_id.clone();
                a.apply_with_checkpoint(&p, |c| {
                    if c == C::Published(0) {
                        Err(skillrail::AppError::new("stop", "fixture"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
                a.apply(&a.plan_undo(&op).unwrap()).unwrap();
                a.apply(&a.plan_undo(&op).unwrap()).unwrap();
            }
            Err(_) => {
                assert_eq!(size, 530_000);
                assert!(!f.path("state/journal").exists());
                assert!(!f.path("home/.skillrail-sources").exists());
            }
        }
    }
}
#[cfg(target_os = "linux")]
#[test]
fn actual_process_death_during_partial_snapshot_write_recovers() {
    const CHILD: &str = "SKILLRAIL_M5_PARTIAL_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        let root = PathBuf::from(root);
        let a = skillrail::Application::new(
            skillrail::Paths::resolve(
                Some(root.join("home")),
                Some(root.join("project")),
                Some(root.join("state")),
            )
            .unwrap(),
        );
        let r = skillrail::mutation::InstallRequest {
            source: root.join("source").to_str().unwrap().into(),
            git: false,
            revision: None,
            skill: None,
            agents: vec!["codex".into()],
            scope: "user".into(),
        };
        a.apply(&a.plan_install(r).unwrap()).unwrap();
        return;
    }
    let f = Fixture::new();
    let code = f.path("fault.c");
    let library = f.path("fault.so");
    fs::write(&code,r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
ssize_t write(int fd,const void *buf,size_t count){
 ssize_t (*real_write)(int,const void*,size_t)=dlsym(RTLD_NEXT,"write");
 char name[80],path[4096];snprintf(name,sizeof(name),"/proc/self/fd/%d",fd);
 ssize_t n=readlink(name,path,sizeof(path)-1);const char *root=getenv("SKILLRAIL_M5_PARTIAL_CHILD");
 if(n>0&&root&&count>10){path[n]=0;if(strncmp(path,root,strlen(root))==0&&strstr(path,"/.skillrail-sources/.skillrail-")&&strstr(path,"/SKILL.md")){real_write(fd,buf,count/2);_exit(79);}}
 return real_write(fd,buf,count);
}
"#).unwrap();
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC"])
            .arg(code)
            .args(["-ldl", "-o"])
            .arg(&library)
            .output()
            .unwrap()
            .status
            .success()
    );
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "actual_process_death_during_partial_snapshot_write_recovers",
            "--nocapture",
        ])
        .env(CHILD, f.root.path())
        .env("LD_PRELOAD", library)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(79),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let a = app(&f);
    let h = a.operations().unwrap();
    assert_eq!(h.operations.len(), 1);
    let op = &h.operations[0].operation_id;
    assert_eq!(h.operations[0].status, "prepared");
    a.apply(&a.plan_undo(op).unwrap()).unwrap();
    a.apply(&a.plan_undo(op).unwrap()).unwrap();
    assert_eq!(
        fs::read_dir(f.path("home/.skillrail-sources"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn malformed_owned_manifest_and_journal_cannot_publish_credential_provenance() {
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    let op = a.apply(&p).unwrap().operation_id;
    let manifest = p.view().files[0].join(".skillrail/manifest.json");
    let original = fs::read(&manifest).unwrap();
    let mut m: Value = serde_json::from_slice(&original).unwrap();
    m["provenance"]["kind"] = "git".into();
    m["provenance"]["revision"] = "a".repeat(40).into();
    m["provenance"]["origin"] = "https://user:OWNED_URL_CANARY@example.test/repo".into();
    fs::write(&manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let v = json(f.run(&["scan", "--json"]));
    assert!(!v.to_string().contains("OWNED_URL_CANARY"));
    let path = f.path(&format!("state/journal/{op}.json"));
    let mut r: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    r["install"]["manifest"] = m;
    fs::write(path, serde_json::to_vec(&r).unwrap()).unwrap();
    let out = f.run(&["undo", &op, "--json"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("OWNED_URL_CANARY"));
}
#[test]
fn corrupt_prepublication_record_refuses_before_preview_serialization() {
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    let op = p.view().operation_id.clone();
    a.apply_with_checkpoint(&p, |c| {
        if c == skillrail::mutation::Checkpoint::Recorded {
            Err(skillrail::AppError::new("stop", "fixture"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    let path = f.path(&format!("state/journal/{op}.json"));
    let mut r: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    r["install"]["manifest"]["provenance"]["origin"] =
        "https://user:JOURNAL_URL_CANARY@example.test/repo".into();
    fs::write(path, serde_json::to_vec(&r).unwrap()).unwrap();
    let out = f.run(&["undo", &op, "--json"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("JOURNAL_URL_CANARY"));
}
#[tokio::test]
async fn installed_inventory_mcp_and_tui_share_provenance_without_serving_bodies() {
    use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    a.apply(&p).unwrap();
    let inventory = a.scan().unwrap();
    let details = skillrail::presentation::details(&inventory, 0);
    let hash = inventory.items[0]
        .source
        .installed
        .as_ref()
        .unwrap()
        .content_hash
        .clone();
    assert!(details.contains(&hash));
    assert!(!details.contains("BODY_CANARY"));
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_skillrail"));
    cmd.arg("--home")
        .arg(f.path("home"))
        .arg("--project")
        .arg(f.path("project"))
        .arg("--state-dir")
        .arg(f.path("state"))
        .arg("mcp");
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let client = ().serve(TokioChildProcess::new(cmd).unwrap()).await.unwrap();
        assert_eq!(client.list_tools(None).await.unwrap().tools.len(), 5);
        let result = client
            .call_tool(CallToolRequestParams::new("list_inventory"))
            .await
            .unwrap();
        let text = &result.content[0].as_text().unwrap().text;
        assert!(text.contains(&hash));
        assert!(text.contains("skillrail"));
        assert!(!text.contains("BODY_CANARY"));
        client.cancel().await.unwrap();
    })
    .await
    .unwrap();
    assert!(
        !fs::read(f.path("state/index.sqlite3"))
            .unwrap()
            .windows(11)
            .any(|w| w == b"BODY_CANARY")
    );
}
#[test]
fn undo_refuses_unknown_file_consumers_in_agent_locations() {
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    let op = a.apply(&p).unwrap().operation_id;
    std::os::unix::fs::symlink(
        p.view().files[0].join("SKILL.md"),
        f.path("home/.agents/skills/file-alias"),
    )
    .unwrap();
    assert!(a.plan_undo(&op).is_err());
    assert!(p.view().files[0].exists());
}
#[test]
fn undo_refuses_replaced_empty_owned_directory() {
    let f = Fixture::new();
    fs::create_dir(f.path("source/empty")).unwrap();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    let op = a.apply(&p).unwrap().operation_id;
    let path = p.view().files[0].join("empty");
    fs::rename(&path, f.path("moved-empty")).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(a.plan_undo(&op).is_err());
    assert!(path.exists());
}
#[test]
fn install_plan_is_bound_to_its_selected_private_journal() {
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    let b = skillrail::Application::new(
        skillrail::Paths::resolve(
            Some(f.path("home")),
            Some(f.path("project")),
            Some(f.path("other-state")),
        )
        .unwrap(),
    );
    assert!(b.apply(&p).is_err());
    assert!(!f.path("home/.skillrail-sources").exists());
}
#[test]
fn local_and_git_selected_skill_support_each_agent_and_scope() {
    for is_git in [false, true] {
        for agent in ["claude", "codex"] {
            for scope in ["user", "project"] {
                let f = Fixture::new();
                fs::create_dir_all(f.path("source/skills/chosen")).unwrap();
                fs::write(
                    f.path("source/skills/chosen/SKILL.md"),
                    "---\nname: chosen\ndescription: chosen fixture\n---\n",
                )
                .unwrap();
                if is_git {
                    git(&f, &["init", "--template="]);
                    git(&f, &["add", "."]);
                    git(
                        &f,
                        &[
                            "-c",
                            "user.name=Fixture",
                            "-c",
                            "user.email=fixture@example.test",
                            "commit",
                            "-m",
                            "fixture",
                        ],
                    );
                }
                let source = f.path("source");
                let mut args = vec![
                    "install",
                    source.to_str().unwrap(),
                    "--agent",
                    agent,
                    "--scope",
                    scope,
                    "--skill",
                    "skills/chosen",
                    "--apply",
                    "--json",
                ];
                if is_git {
                    args.push("--git");
                }
                let v = json(f.run(&args));
                assert_eq!(v["install"]["provenance"]["subpath"], "skills/chosen");
                let base = if scope == "user" { "home" } else { "project" };
                let native = if agent == "claude" {
                    ".claude"
                } else {
                    ".agents"
                };
                let link = f.path(&format!("{base}/{native}/skills/chosen"));
                assert!(link.is_symlink());
                let inventory = json(f.run(&["scan", "--json"]));
                assert_eq!(inventory["occurrences"].as_array().unwrap().len(), 1);
                assert_eq!(inventory["occurrences"][0]["agent"], agent);
                assert_eq!(inventory["occurrences"][0]["scope"], scope);
                json(f.run(&[
                    "undo",
                    v["operation_id"].as_str().unwrap(),
                    "--apply",
                    "--json",
                ]));
                assert!(!link.exists());
            }
        }
    }
}
#[test]
fn git_ref_advancing_since_preview_refuses_before_admission() {
    let f = Fixture::new();
    git(&f, &["init", "--template="]);
    git(&f, &["add", "."]);
    git(
        &f,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-m",
            "first",
        ],
    );
    let a = app(&f);
    let mut r = request(&f);
    r.git = true;
    let p = a.plan_install(r).unwrap();
    fs::write(f.path("source/new"), "new revision").unwrap();
    git(&f, &["add", "."]);
    git(
        &f,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-m",
            "second",
        ],
    );
    assert_eq!(a.apply(&p).unwrap_err().code, "mutation_conflict");
    assert!(!f.path("state/journal").exists());
}
