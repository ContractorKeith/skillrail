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
fn git(f: &Fixture, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(f.path("source"))
        .args(["-c", "core.hooksPath=/dev/null"])
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
fn owned_update_full_folder_diff_noop_uninstall_and_precise_undo() {
    let f = Fixture::new();
    fs::write(f.path("source/asset"), [0, 255, 1]).unwrap();
    let installed = json(f.run(&[
        "install",
        f.path("source").to_str().unwrap(),
        "--agent",
        "codex",
        "--scope",
        "user",
        "--apply",
        "--json",
    ]));
    let owned = installed["install"]["owned_id"].as_str().unwrap();
    let original = fs::read_link(f.path("home/.agents/skills/demo")).unwrap();
    assert_eq!(
        json(f.run(&["update", owned, "--apply", "--json"]))["status"],
        "unchanged"
    );
    fs::write(f.path("source/asset"), [0, 255, 2, 3]).unwrap();
    fs::create_dir(f.path("source/empty")).unwrap();
    let preview = json(f.run(&["update", owned, "--json"]));
    assert_eq!(preview["action"], "update");
    assert_eq!(preview["content_diff"].as_array().unwrap().len(), 2);
    assert_eq!(preview["content_diff"][0]["path"], "asset");
    assert_eq!(preview["content_diff"][0]["before"]["size"], 3);
    assert_eq!(preview["content_diff"][0]["after"]["size"], 4);
    assert!(!preview.to_string().contains("BODY_CANARY"));
    assert_eq!(
        fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
        original
    );
    let updated = json(f.run(&[
        "update",
        owned,
        "--apply",
        "--expect",
        preview["token"].as_str().unwrap(),
        "--json",
    ]));
    assert_eq!(
        fs::read(f.path("home/.agents/skills/demo/asset")).unwrap(),
        [0, 255, 2, 3]
    );
    json(f.run(&[
        "undo",
        updated["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]));
    assert_eq!(
        fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
        original
    );
    assert_eq!(fs::read(original.join("asset")).unwrap(), [0, 255, 1]);
    let removed = json(f.run(&["uninstall", owned, "--apply", "--json"]));
    assert!(!original.exists());
    assert!(!f.path("home/.agents/skills/demo").exists());
    json(f.run(&[
        "undo",
        removed["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]));
    assert_eq!(
        fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
        original
    );
    json(f.run(&[
        "undo",
        installed["operation_id"].as_str().unwrap(),
        "--apply",
        "--json",
    ]));
}

#[test]
fn lifecycle_interruption_at_each_publication_and_repeated_undo_restores_original_inodes() {
    use skillrail::mutation::Checkpoint as C;
    use std::os::unix::fs::MetadataExt;
    for update in [false, true] {
        let mut stops = vec![
            C::Recorded,
            C::BeforeRetain(1),
            C::Retained(1),
            C::Retained(2),
            C::BeforeRetain(0),
            C::Retained(0),
        ];
        if update {
            stops.extend([
                C::TreeFile(0),
                C::Staged(0),
                C::Published(0),
                C::Staged(1),
                C::Published(1),
                C::Published(2),
            ]);
        }
        for stop in stops {
            let f = Fixture::new();
            let a = app(&f);
            let installed = a.plan_install(request(&f)).unwrap();
            a.apply(&installed).unwrap();
            let snapshot = installed.view().files[0].clone();
            let inode = fs::metadata(&snapshot).unwrap().ino();
            let link_inode = fs::symlink_metadata(f.path("home/.agents/skills/demo"))
                .unwrap()
                .ino();
            let owned = snapshot.file_name().unwrap().to_str().unwrap();
            fs::write(f.path("source/new"), "NEW_BODY_CANARY").unwrap();
            let p = if update {
                a.plan_update(owned)
            } else {
                a.plan_uninstall(owned)
            }
            .unwrap();
            let op = p.view().operation_id.clone();
            let err = a
                .apply_with_checkpoint(&p, |c| {
                    if c == stop {
                        Err(skillrail::AppError::new("stop", "fixture"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
            assert_eq!(err.code, "stop", "update={update} stop={stop:?}");
            for _ in 0..4 {
                let u = a
                    .plan_undo(&op)
                    .unwrap_or_else(|e| panic!("update={update} stop={stop:?}: {e}"));
                let _ = a.apply_with_checkpoint(&u, |c| {
                    if matches!(c, C::Published(_) | C::TreeFile(_) | C::Restored(_)) {
                        Err(skillrail::AppError::new("stop", "fixture"))
                    } else {
                        Ok(())
                    }
                });
            }
            a.apply(&a.plan_undo(&op).unwrap()).unwrap();
            assert_eq!(fs::metadata(&snapshot).unwrap().ino(), inode);
            assert_eq!(
                fs::symlink_metadata(f.path("home/.agents/skills/demo"))
                    .unwrap()
                    .ino(),
                link_inode
            );
            assert!(!snapshot.join("new").exists());
        }
    }
}

#[test]
fn lifecycle_update_undo_refuses_unknown_consumers_of_new_snapshot_during_preview() {
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    a.apply(&p).unwrap();
    fs::write(f.path("source/new"), "new").unwrap();
    let p = a
        .plan_update(p.view().files[0].file_name().unwrap().to_str().unwrap())
        .unwrap();
    a.apply(&p).unwrap();
    let new = fs::read_link(f.path("home/.agents/skills/demo")).unwrap();
    std::os::unix::fs::symlink(&new, f.path("home/.agents/skills/unowned")).unwrap();
    assert!(
        a.plan_undo(&p.view().operation_id).is_err(),
        "undo preview accepted an unknown consumer of the new snapshot"
    );
    assert!(new.exists());
}

#[test]
fn lifecycle_retained_update_plan_refuses_identical_source_directory_retarget() {
    let f = Fixture::new();
    let a = app(&f);
    let installed = a.plan_install(request(&f)).unwrap();
    a.apply(&installed).unwrap();
    fs::write(f.path("source/new"), "new").unwrap();
    let p = a
        .plan_update(
            installed.view().files[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
    fs::rename(f.path("source"), f.path("old-source")).unwrap();
    fs::create_dir(f.path("source")).unwrap();
    for name in ["SKILL.md", "new"] {
        fs::copy(
            f.path(&format!("old-source/{name}")),
            f.path(&format!("source/{name}")),
        )
        .unwrap();
    }
    assert!(
        a.apply(&p).is_err(),
        "identical replacement source directory reused a retained preview"
    );
    assert_eq!(
        fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
        installed.view().files[0]
    );
}

#[test]
fn lifecycle_edits_ownership_and_incomplete_consumers_refuse_without_changes() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for kind in [
        "snapshot",
        "manifest",
        "extra",
        "link",
        "missing-link",
        "consumer",
        "file-consumer",
        "incomplete",
        "plugin",
        "case",
        "unicode",
    ] {
        let f = Fixture::new();
        let a = app(&f);
        let p = a.plan_install(request(&f)).unwrap();
        a.apply(&p).unwrap();
        let snapshot = &p.view().files[0];
        let owned = snapshot.file_name().unwrap().to_str().unwrap();
        fs::write(f.path("source/new"), "new").unwrap();
        match kind {
            "snapshot" => fs::write(snapshot.join("SKILL.md"), "external").unwrap(),
            "manifest" => fs::write(snapshot.join(".skillrail/manifest.json"), "{}").unwrap(),
            "extra" => fs::write(snapshot.join("extra"), "external").unwrap(),
            "link" | "missing-link" => {
                fs::remove_file(f.path("home/.agents/skills/demo")).unwrap();
                if kind == "link" {
                    symlink(f.path("source"), f.path("home/.agents/skills/demo")).unwrap();
                }
            }
            "consumer" => symlink(snapshot, f.path("home/.agents/skills/alias")).unwrap(),
            "file-consumer" => symlink(
                snapshot.join("SKILL.md"),
                f.path("home/.agents/skills/alias"),
            )
            .unwrap(),
            "incomplete" => fs::set_permissions(
                f.path("home/.claude/skills"),
                fs::Permissions::from_mode(0o000),
            )
            .unwrap(),
            "case" => {
                // Two renames retain the inode and work on case-insensitive filesystems too.
                fs::rename(f.path("home/.agents/skills/demo"), f.path("held-case-link")).unwrap();
                fs::rename(f.path("held-case-link"), f.path("home/.agents/skills/DEMO")).unwrap();
            }
            "unicode" => {
                fs::create_dir(f.path("home/.agents/skills/démo")).unwrap();
                fs::copy(
                    f.path("source/SKILL.md"),
                    f.path("home/.agents/skills/démo/SKILL.md"),
                )
                .unwrap();
            }
            _ => {
                let plugin = f.path("home/.claude/plugins/cache/market/plug/1.0.0");
                fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
                fs::write(
                    plugin.join(".claude-plugin/plugin.json"),
                    r#"{"name":"plug"}"#,
                )
                .unwrap();
                let skill = plugin.join("skills/external");
                fs::create_dir_all(&skill).unwrap();
                fs::write(
                    skill.join("SKILL.md"),
                    "---\nname: external\ndescription: fixture\n---\n",
                )
                .unwrap();
                symlink(snapshot.join("SKILL.md"), skill.join("reference")).unwrap();
            }
        }
        assert!(a.plan_update(owned).is_err(), "update {kind}");
        assert!(a.plan_uninstall(owned).is_err(), "uninstall {kind}");
        assert!(snapshot.exists());
        if kind == "incomplete" {
            fs::set_permissions(
                f.path("home/.claude/skills"),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        assert_eq!(a.operations().unwrap().operations.len(), 1);
    }
    let f = Fixture::new();
    let a = app(&f);
    fs::create_dir_all(f.path("home/.agents/skills/external")).unwrap();
    fs::copy(
        f.path("source/SKILL.md"),
        f.path("home/.agents/skills/external/SKILL.md"),
    )
    .unwrap();
    let inventory = a.scan().unwrap();
    for id in [&inventory.items[0].id, &inventory.occurrences[0].id] {
        assert!(a.plan_update(id).is_err());
        assert!(a.plan_uninstall(id).is_err());
    }
    assert!(a.plan_update("demo").is_err());
    assert!(a.plan_uninstall("missing").is_err());
}

#[test]
fn lifecycle_git_ref_retarget_token_noop_metadata_and_second_update() {
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
    a.apply(&p).unwrap();
    let snapshot = &p.view().files[0];
    let owned = snapshot.file_name().unwrap().to_str().unwrap();
    let inventory = a.scan().unwrap();
    assert_eq!(
        a.apply(&a.plan_update(&inventory.items[0].id).unwrap())
            .unwrap()
            .status,
        "unchanged"
    );
    fs::write(f.path("source/new"), "new").unwrap();
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
    let preview = json(f.run(&["update", owned, "--json"]));
    let p = a.plan_update(owned).unwrap();
    git(
        &f,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "retarget",
        ],
    );
    assert!(a.apply(&p).is_err());
    assert_eq!(
        f.run(&[
            "update",
            owned,
            "--apply",
            "--expect",
            preview["token"].as_str().unwrap(),
            "--json"
        ])
        .status
        .code(),
        Some(4)
    );
    let p = a.plan_update(owned).unwrap();
    a.apply(&p).unwrap();
    let current = fs::read_link(f.path("home/.agents/skills/demo")).unwrap();
    let id = current.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        a.apply(&a.plan_update(id).unwrap()).unwrap().status,
        "unchanged"
    );
    let un = a.plan_uninstall(id).unwrap();
    a.apply(&un).unwrap();
    a.apply(&a.plan_undo(&un.view().operation_id).unwrap())
        .unwrap();
    assert_eq!(
        fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
        current
    );
    a.apply(&a.plan_undo(&p.view().operation_id).unwrap())
        .unwrap();
    assert_eq!(
        fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
        *snapshot
    );
}

#[test]
fn lifecycle_stale_plans_boundary_substitutions_and_undo_edits_preserve_external_bytes() {
    use skillrail::mutation::Checkpoint as C;
    for update in [false, true] {
        for target in ["snapshot", "link", "backup", "consumer"] {
            let f = Fixture::new();
            let a = app(&f);
            let p = a.plan_install(request(&f)).unwrap();
            a.apply(&p).unwrap();
            let snapshot = &p.view().files[0];
            let owned = snapshot.file_name().unwrap().to_str().unwrap();
            fs::write(f.path("source/new"), "new").unwrap();
            let p = if update {
                a.plan_update(owned)
            } else {
                a.plan_uninstall(owned)
            }
            .unwrap();
            let result = a.apply_with_checkpoint(&p, |point| {
                if point == C::BeforeRetain(1) {
                    match target {
                        "snapshot" => {
                            fs::write(snapshot.join("SKILL.md"), "EXTERNAL_BYTES").unwrap()
                        }
                        "link" => {
                            fs::rename(f.path("home/.claude/skills/demo"), f.path("held-link"))
                                .unwrap();
                            std::os::unix::fs::symlink(
                                f.path("source"),
                                f.path("home/.claude/skills/demo"),
                            )
                            .unwrap();
                        }
                        "consumer" => std::os::unix::fs::symlink(
                            snapshot,
                            f.path("home/.agents/skills/unknown"),
                        )
                        .unwrap(),
                        _ => {
                            let dir = f
                                .path("home/.claude/skills")
                                .join(format!(".skillrail-{}-original", p.view().operation_id));
                            fs::rename(&dir, f.path("held-backup")).unwrap();
                            std::os::unix::fs::symlink(f.path("held-backup"), dir).unwrap();
                        }
                    }
                }
                Ok(())
            });
            assert!(result.is_err(), "update={update} target={target}");
            assert!(snapshot.exists());
        }
        for target in ["backup", "link", "new"] {
            if !update && target == "new" {
                continue;
            }
            let f = Fixture::new();
            let a = app(&f);
            let p = a.plan_install(request(&f)).unwrap();
            a.apply(&p).unwrap();
            let owned = p.view().files[0].file_name().unwrap().to_str().unwrap();
            fs::write(f.path("source/new"), "new").unwrap();
            let p = if update {
                a.plan_update(owned)
            } else {
                a.plan_uninstall(owned)
            }
            .unwrap();
            a.apply(&p).unwrap();
            let changed = match target {
                "backup" => f.path("home/.skillrail-sources").join(format!(
                    ".skillrail-{}-original/SKILL.md",
                    p.view().operation_id
                )),
                "link" => {
                    if update {
                        fs::rename(f.path("home/.agents/skills/demo"), f.path("held-link"))
                            .unwrap();
                    }
                    fs::write(f.path("home/.agents/skills/demo"), "EXTERNAL_BYTES").unwrap();
                    f.path("home/.agents/skills/demo")
                }
                _ => f.path("home/.agents/skills/demo/new"),
            };
            fs::write(&changed, "EXTERNAL_BYTES").unwrap();
            assert!(a.plan_undo(&p.view().operation_id).is_err());
            assert_eq!(fs::read(changed).unwrap(), b"EXTERNAL_BYTES");
        }
    }
}

#[test]
fn lifecycle_complete_budget_admission_retention_and_cross_process_lock() {
    for size in [200_000, 524_200] {
        let f = Fixture::new();
        let a = app(&f);
        fs::write(f.path("source/blob"), vec![255u8; size]).unwrap();
        let p = a.plan_install(request(&f)).unwrap();
        a.apply(&p).unwrap();
        let snapshot = &p.view().files[0];
        let owned = snapshot.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            a.apply(&a.plan_update(owned).unwrap()).unwrap().status,
            "unchanged"
        );
        fs::write(f.path("source/blob"), vec![254u8; size]).unwrap();
        let result = a.plan_update(owned);
        if size == 524_200 {
            assert!(result.is_err());
            assert_eq!(a.operations().unwrap().operations.len(), 1);
            assert!(snapshot.exists());
        } else {
            let p = result.unwrap();
            a.apply_with_checkpoint(&p, |point| {
                if point == skillrail::mutation::Checkpoint::Locked {
                    assert_eq!(
                        f.run(&["uninstall", owned, "--apply", "--json"])
                            .status
                            .code(),
                        Some(4)
                    );
                }
                Ok(())
            })
            .unwrap();
            let backup = f
                .path("home/.skillrail-sources")
                .join(format!(".skillrail-{}-original", p.view().operation_id));
            assert_eq!(fs::read(backup.join("blob")).unwrap(), vec![255u8; size]);
            a.apply(&a.plan_undo(&p.view().operation_id).unwrap())
                .unwrap();
            assert_eq!(fs::read(snapshot.join("blob")).unwrap(), vec![255u8; size]);
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn lifecycle_real_process_death_midwrite_before_stage_identity_and_repeated_restore() {
    let f = Fixture::new();
    let code = f.path("fault.c");
    let library = f.path("fault.so");
    fs::write(&code, r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
static int mode(const char *s){const char *v=getenv("SKILLRAIL_M6_DEATH");return v&&strcmp(v,s)==0;}
static int path_for(int fd,char *path){char name[80];snprintf(name,sizeof(name),"/proc/self/fd/%d",fd);ssize_t n=readlink(name,path,4095);if(n<0)return 0;path[n]=0;const char *root=getenv("SKILLRAIL_M6_ROOT");return root&&strncmp(path,root,strlen(root))==0;}
ssize_t write(int fd,const void *buf,size_t count){
 ssize_t (*real_write)(int,const void*,size_t)=dlsym(RTLD_NEXT,"write");char path[4096];
 if(mode("write")&&count>10&&path_for(fd,path)&&strstr(path,"/.skillrail-sources/.skillrail-")&&strstr(path,"-install/SKILL.md")){real_write(fd,buf,count/2);_exit(79);}return real_write(fd,buf,count);
}
int fsync(int fd){int (*real_fsync)(int)=dlsym(RTLD_NEXT,"fsync");int r=real_fsync(fd);char path[4096];if(r==0&&mode("stage")&&path_for(fd,path)&&strstr(path,"/.skillrail-sources")&&strcmp(strrchr(path,'/'),"/.skillrail-sources")==0){DIR *d=opendir(path);if(d){struct dirent *e;while((e=readdir(d))){if(strstr(e->d_name,".skillrail-op-")&&strstr(e->d_name,"-install"))_exit(80);}closedir(d);}}return r;}
int renameat2(int a,const char *s,int b,const char *t,unsigned flags){int (*real_rename)(int,const char*,int,const char*,unsigned)=dlsym(RTLD_NEXT,"renameat2");int r=real_rename(a,s,b,t,flags);char path[4096];if(r==0&&path_for(a,path)){if(mode("retain")&&strstr(t,"-original"))_exit(81);if(mode("restore")&&(strstr(s,"-original")||strcmp(s,"link")==0))_exit(82);if(mode("publish")&&strstr(s,"-install")&&strlen(t)==64)_exit(83);}return r;}
"#).unwrap();
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC"])
            .arg(code)
            .args(["-ldl", "-o"])
            .arg(&library)
            .status()
            .unwrap()
            .success()
    );
    for (death, expected) in [
        ("write", 79),
        ("stage", 80),
        ("retain", 81),
        ("publish", 83),
    ] {
        for update in [true, false] {
            if !update && death != "retain" {
                continue;
            }
            let f = Fixture::new();
            let a = app(&f);
            let p = a.plan_install(request(&f)).unwrap();
            a.apply(&p).unwrap();
            let snapshot = &p.view().files[0];
            let owned = snapshot.file_name().unwrap().to_str().unwrap();
            fs::write(f.path("source/new"), "new").unwrap();
            let run = |args: &[&str], death: &str| {
                Command::new(env!("CARGO_BIN_EXE_skillrail"))
                    .arg("--home")
                    .arg(f.path("home"))
                    .arg("--project")
                    .arg(f.path("project"))
                    .arg("--state-dir")
                    .arg(f.path("state"))
                    .args(args)
                    .env("LD_PRELOAD", &library)
                    .env("SKILLRAIL_M6_DEATH", death)
                    .env("SKILLRAIL_M6_ROOT", f.root.path())
                    .output()
                    .unwrap()
            };
            let out = run(
                &[
                    if update { "update" } else { "uninstall" },
                    owned,
                    "--apply",
                    "--json",
                ],
                death,
            );
            assert_eq!(
                out.status.code(),
                Some(expected),
                "{death}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let h = a.operations().unwrap();
            let op = &h
                .operations
                .iter()
                .find(|o| o.status == "prepared")
                .unwrap()
                .operation_id;
            // Every original rename is interrupted in a different real process; retries preserve progress.
            for _ in 0..4 {
                let out = run(&["undo", op, "--apply", "--json"], "restore");
                assert!(
                    out.status.success() || out.status.code() == Some(82),
                    "{death}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            a.apply(&a.plan_undo(op).unwrap()).unwrap();
            assert!(snapshot.exists());
            assert!(!snapshot.join("new").exists());
            assert_eq!(
                fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
                *snapshot
            );
        }
    }
}

#[test]
fn lifecycle_completed_update_undo_requires_complete_postconditions() {
    for missing in ["link", "snapshot"] {
        let f = Fixture::new();
        let a = app(&f);
        let installed = a.plan_install(request(&f)).unwrap();
        a.apply(&installed).unwrap();
        fs::write(f.path("source/new"), "new").unwrap();
        let p = a
            .plan_update(
                installed.view().files[0]
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
        a.apply(&p).unwrap();
        if missing == "link" {
            fs::rename(f.path("home/.agents/skills/demo"), f.path("held-link")).unwrap();
        } else {
            let snapshot = fs::read_link(f.path("home/.agents/skills/demo")).unwrap();
            fs::rename(snapshot, f.path("held-snapshot")).unwrap();
        }
        assert!(
            a.plan_undo(&p.view().operation_id).is_err(),
            "completed update accepted missing {missing}"
        );
    }
}

#[test]
fn lifecycle_unknown_consumers_of_private_backup_and_stage_refuse_recovery() {
    use skillrail::mutation::Checkpoint as C;
    for target in ["backup", "stage"] {
        let f = Fixture::new();
        let a = app(&f);
        let installed = a.plan_install(request(&f)).unwrap();
        a.apply(&installed).unwrap();
        fs::write(f.path("source/new"), "new").unwrap();
        let p = a
            .plan_update(
                installed.view().files[0]
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
        if target == "backup" {
            a.apply(&p).unwrap();
        } else {
            a.apply_with_checkpoint(&p, |c| {
                if c == C::Staged(0) {
                    Err(skillrail::AppError::new("stop", "fixture"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        }
        let private = f.path("home/.skillrail-sources").join(format!(
            ".skillrail-{}-{}",
            p.view().operation_id,
            if target == "backup" {
                "original"
            } else {
                "install"
            }
        ));
        std::os::unix::fs::symlink(&private, f.path("home/.agents/skills/unknown")).unwrap();
        assert!(
            a.plan_undo(&p.view().operation_id).is_err(),
            "unknown {target} consumer was accepted"
        );
        assert!(private.exists());
    }
}

#[test]
fn lifecycle_source_changes_at_snapshot_publication_refuse_and_recover() {
    let f = Fixture::new();
    let a = app(&f);
    let installed = a.plan_install(request(&f)).unwrap();
    a.apply(&installed).unwrap();
    fs::write(f.path("source/new"), "planned").unwrap();
    let p = a
        .plan_update(
            installed.view().files[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
    let result = a.apply_with_checkpoint(&p, |c| {
        if c == skillrail::mutation::Checkpoint::BeforePublish(0) {
            fs::write(f.path("source/new"), "changed after staging").unwrap();
        }
        Ok(())
    });
    assert!(
        result.is_err(),
        "source changed at publication but applied anyway"
    );
    a.apply(&a.plan_undo(&p.view().operation_id).unwrap())
        .unwrap();
    assert_eq!(
        fs::read_link(f.path("home/.agents/skills/demo")).unwrap(),
        installed.view().files[0]
    );
}

#[test]
fn lifecycle_diff_includes_removed_added_binary_directory_and_mode_only_changes() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let a = app(&f);
    fs::write(f.path("source/removed"), "OLD_PRIVATE_PAYLOAD").unwrap();
    fs::write(f.path("source/script"), "#!/bin/sh\nexit 27\n").unwrap();
    let p = a.plan_install(request(&f)).unwrap();
    a.apply(&p).unwrap();
    fs::remove_file(f.path("source/removed")).unwrap();
    fs::write(f.path("source/binary"), [0, 255, 0]).unwrap();
    fs::create_dir(f.path("source/empty")).unwrap();
    fs::set_permissions(f.path("source/script"), fs::Permissions::from_mode(0o700)).unwrap();
    let p = a
        .plan_update(p.view().files[0].file_name().unwrap().to_str().unwrap())
        .unwrap();
    let v = serde_json::to_value(p.view()).unwrap();
    assert_eq!(v["content_diff"].as_array().unwrap().len(), 4);
    let mode = v["content_diff"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["path"] == "script")
        .unwrap();
    assert_eq!(mode["before"]["sha256"], mode["after"]["sha256"]);
    assert_eq!(mode["before"]["mode"], 0o600);
    assert_eq!(mode["after"]["mode"], 0o700);
    assert!(!v.to_string().contains("OLD_PRIVATE_PAYLOAD"));
    a.apply(&p).unwrap();
    assert!(f.path("home/.agents/skills/demo/empty").is_dir());
    a.apply(&a.plan_undo(&p.view().operation_id).unwrap())
        .unwrap();
    assert_eq!(
        fs::read(f.path("home/.agents/skills/demo/removed")).unwrap(),
        b"OLD_PRIVATE_PAYLOAD"
    );
}

#[tokio::test]
async fn lifecycle_updated_mcp_inventory_remains_read_only_and_body_free() {
    use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    a.apply(&p).unwrap();
    fs::write(f.path("source/new"), "M6_NEW_SECRET_PAYLOAD").unwrap();
    let p = a
        .plan_update(p.view().files[0].file_name().unwrap().to_str().unwrap())
        .unwrap();
    a.apply(&p).unwrap();
    let inventory = a.scan().unwrap();
    let v = serde_json::to_string(&inventory).unwrap();
    assert!(!v.contains("M6_NEW_SECRET_PAYLOAD") && !v.contains("BODY_CANARY"));
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
        assert!(text.contains("owned_snapshot_manifest"));
        assert!(!text.contains("M6_NEW_SECRET_PAYLOAD") && !text.contains("BODY_CANARY"));
        client.cancel().await.unwrap();
    })
    .await
    .unwrap();
    assert!(
        !fs::read(f.path("state/index.sqlite3"))
            .unwrap()
            .windows(21)
            .any(|w| w == b"M6_NEW_SECRET_PAYLOAD")
    );
}

#[test]
fn lifecycle_retained_original_prevents_older_install_undo_from_stranding_ownership() {
    let f = Fixture::new();
    let a = app(&f);
    let p = a.plan_install(request(&f)).unwrap();
    a.apply(&p).unwrap();
    let owned = p.view().files[0].file_name().unwrap().to_str().unwrap();
    let un = a.plan_uninstall(owned).unwrap();
    a.apply(&un).unwrap();
    assert!(
        a.plan_undo(&p.view().operation_id).is_err(),
        "older install was marked undone while its snapshot was retained by uninstall"
    );
    a.apply(&a.plan_undo(&un.view().operation_id).unwrap())
        .unwrap();
    assert!(a.plan_update(owned).is_ok());
    a.apply(&a.plan_undo(&p.view().operation_id).unwrap())
        .unwrap();
}

#[test]
fn lifecycle_reserves_agent_root_capacity_before_retaining_originals() {
    let f = Fixture::new();
    let a = app(&f);
    let root = f.path("home/.agents/skills");
    fs::create_dir_all(&root).unwrap();
    for n in 0..9998 {
        fs::write(root.join(format!("filler{n:05}")), "").unwrap();
    }
    let mut r = request(&f);
    r.agents = vec!["codex".into()];
    let installed = a.plan_install(r).unwrap();
    a.apply(&installed).unwrap();
    let owned = installed.view().files[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    fs::write(f.path("source/new"), "new").unwrap();
    assert!(a.plan_update(owned).is_err());
    assert!(installed.view().files[0].exists());
    assert!(root.join("demo").exists());
    assert_eq!(a.operations().unwrap().operations.len(), 1);
    let un = a.plan_uninstall(owned).unwrap();
    a.apply(&un).unwrap();
    a.apply(&a.plan_undo(&un.view().operation_id).unwrap())
        .unwrap();
    assert!(root.join("demo").exists());
}

#[test]
fn lifecycle_git_update_never_executes_source_filters_hooks_or_skill_scripts() {
    use std::os::unix::fs::PermissionsExt;
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
    let installed = a.plan_install(r).unwrap();
    a.apply(&installed).unwrap();
    fs::write(f.path("source/.gitattributes"), "SKILL.md filter=evil\n").unwrap();
    fs::write(
        f.path("source/script"),
        format!("#!/bin/sh\ntouch {}\n", f.path("executed").display()),
    )
    .unwrap();
    fs::set_permissions(f.path("source/script"), fs::Permissions::from_mode(0o700)).unwrap();
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
    let command = format!("touch {}", f.path("executed").display());
    git(&f, &["config", "filter.evil.smudge", &command]);
    git(&f, &["config", "core.fsmonitor", &command]);
    fs::create_dir_all(f.path("source/.git/hooks")).unwrap();
    fs::write(
        f.path("source/.git/hooks/post-checkout"),
        format!("#!/bin/sh\n{command}\n"),
    )
    .unwrap();
    fs::set_permissions(
        f.path("source/.git/hooks/post-checkout"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let p = a
        .plan_update(
            installed.view().files[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
    a.apply(&p).unwrap();
    a.scan().unwrap();
    assert!(!f.path("executed").exists());
    a.apply(&a.plan_undo(&p.view().operation_id).unwrap())
        .unwrap();
    assert!(!f.path("executed").exists());
}
