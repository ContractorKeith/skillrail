use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use skillrail::{Application, Paths, tui};

#[test]
fn drift_toggle_matches_cli_and_clears_without_changing_snapshot() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    for name in ["alpha", "beta"] {
        let folder = root.path().join(".agents/skills").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Fixture\n---\n"),
        )
        .unwrap();
    }
    std::os::unix::fs::symlink(
        root.path().join("missing"),
        root.path().join(".agents/skills/beta/broken"),
    )
    .unwrap();
    let app = Application::new(
        Paths::resolve(
            Some(root.path().into()),
            Some(root.path().into()),
            Some(root.path().join("state")),
        )
        .unwrap(),
    );
    let inventory = app.scan().unwrap();
    let mut view = tui::View::default();
    let key = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    view.handle(&key('d'), &inventory);
    assert!(view.query.drift_only);
    let selected: Vec<_> = view
        .query
        .indices(&inventory)
        .into_iter()
        .map(|i| inventory.items[i].id.clone())
        .collect();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_skillrail"))
        .arg("--home")
        .arg(root.path())
        .arg("--project")
        .arg(root.path())
        .arg("--state-dir")
        .arg(root.path().join("state"))
        .args(["drift", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let ids: Vec<_> = cli["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(selected, ids);
    assert_eq!(selected.len(), 1);
    assert_eq!(cli["items"][0]["name"], "beta");
    let mut terminal = Terminal::new(TestBackend::new(150, 35)).unwrap();
    terminal
        .draw(|frame| view.render(frame, &inventory))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("drift: drifted/unknown"));
    view.handle(&key('d'), &inventory);
    assert_eq!(view.query.indices(&inventory).len(), 2);
    view.handle(&key('d'), &inventory);
    view.handle(&key('x'), &inventory);
    assert!(!view.query.drift_only);
    view.handle(&key('/'), &inventory);
    view.handle(&key('d'), &inventory);
    assert_eq!(view.query.query.as_deref(), Some("d"));
    assert!(!view.query.drift_only);
}

#[test]
fn shared_inventory_renders_in_small_and_resized_terminal_and_quits() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::resolve(
        Some(root.path().into()),
        Some(root.path().into()),
        Some(root.path().join("state")),
    )
    .unwrap();
    let inventory = Application::new(paths).scan().unwrap();
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| tui::render(frame, &inventory))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("No skills or MCP entries found"));
    assert!(text.contains("partial coverage"));
    terminal.backend_mut().resize(8, 3);
    terminal.autoresize().unwrap();
    terminal
        .draw(|frame| tui::render(frame, &inventory))
        .unwrap();
    assert!(tui::should_quit(&Event::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE
    ))));
    assert!(tui::should_quit(&Event::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE
    ))));
    assert!(tui::should_quit(&Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    ))));
    assert!(!tui::should_quit(&Event::Resize(8, 3)));
    assert!(!tui::should_quit(&Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Release
    ))));
}

#[test]
fn fixture_cli_and_tui_use_identical_inventory_and_escape_untrusted_details() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    for name in ["first", "second"] {
        let folder = home.join(".agents/skills").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: \"untrusted\\e[2Jtext\"\n---\n"),
        )
        .unwrap();
    }
    let paths = Paths::resolve(
        Some(home.clone()),
        Some(project.clone()),
        Some(root.path().join("state")),
    )
    .unwrap();
    let inventory = Application::new(paths).scan().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_skillrail"))
        .arg("--home")
        .arg(&home)
        .arg("--project")
        .arg(&project)
        .arg("--state-dir")
        .arg(root.path().join("state"))
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let mut cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut tui_inventory = serde_json::to_value(&inventory).unwrap();
    cli.as_object_mut().unwrap().remove("scanned_at_unix_ms");
    cli.as_object_mut().unwrap().remove("page");
    tui_inventory
        .as_object_mut()
        .unwrap()
        .remove("scanned_at_unix_ms");
    assert_eq!(cli, tui_inventory);
    let mut selection = tui::Selection::default();
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    selection.handle(&key(KeyCode::Down), inventory.items.len());
    assert_eq!(selection.item, 1);
    selection.handle(&key(KeyCode::Down), inventory.items.len());
    assert_eq!(selection.item, 1);
    selection.handle(&key(KeyCode::PageDown), inventory.items.len());
    assert_eq!(selection.scroll, 5);
    selection.handle(&key(KeyCode::PageUp), inventory.items.len());
    assert_eq!(selection.scroll, 0);
    let mut terminal = Terminal::new(TestBackend::new(180, 70)).unwrap();
    terminal
        .draw(|frame| tui::render_selection(frame, &inventory, selection.item, selection.scroll))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains(&inventory.items[1].name));
    assert!(text.contains("Canonical:"));
    assert!(text.contains("Visibility: unknown"));
    assert!(text.contains("Fingerprint:"));
    assert!(text.contains("Invocation:"));
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("\\u{1b}[2J"));
    selection.handle(&key(KeyCode::Up), inventory.items.len());
    assert_eq!(selection.item, 0);
}

#[test]
fn search_filters_details_and_refresh_use_one_timestamped_snapshot() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    for name in ["alpha", "beta"] {
        let folder = root.path().join(".agents/skills").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Fixture {name}\n---\n"),
        )
        .unwrap();
    }
    let app = Application::new(
        Paths::resolve(
            Some(root.path().into()),
            Some(root.path().into()),
            Some(root.path().join("state")),
        )
        .unwrap(),
    );
    let inventory = app.scan().unwrap();
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    let mut view = tui::View::default();
    view.handle(&key(KeyCode::Char('/')), &inventory);
    for c in "beta".chars() {
        assert_eq!(
            view.handle(&key(KeyCode::Char(c)), &inventory),
            tui::Action::Continue
        );
    }
    view.handle(&key(KeyCode::Enter), &inventory);
    let matches = view.query.indices(&inventory);
    assert_eq!(matches.len(), 1);
    assert_eq!(inventory.items[matches[0]].name, "beta");
    let mut terminal = Terminal::new(TestBackend::new(150, 35)).unwrap();
    terminal
        .draw(|frame| view.render(frame, &inventory))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Search: beta"));
    assert!(text.contains("Matches: 1"));
    assert!(text.contains("Visibility: unknown"));
    assert!(text.contains(&inventory.scanned_at_unix_ms.to_string()));
    view.handle(&key(KeyCode::Char('a')), &inventory);
    assert!(view.query.indices(&inventory).is_empty());
    view.handle(&key(KeyCode::Char('a')), &inventory);
    assert_eq!(view.query.indices(&inventory).len(), 1);
    view.handle(&key(KeyCode::Char('k')), &inventory);
    view.handle(&key(KeyCode::Char('k')), &inventory);
    assert!(view.query.indices(&inventory).is_empty());
    view.handle(&key(KeyCode::Char('x')), &inventory);
    assert_eq!(view.query.indices(&inventory).len(), 2);
    assert_eq!(
        view.handle(&key(KeyCode::Char('r')), &inventory),
        tui::Action::Refresh
    );
    assert_eq!(
        view.handle(&Event::Resize(8, 3), &inventory),
        tui::Action::Continue
    );
    terminal.backend_mut().resize(8, 3);
    terminal.autoresize().unwrap();
    terminal
        .draw(|frame| view.render(frame, &inventory))
        .unwrap();
    view.handle(&key(KeyCode::Char('/')), &inventory);
    view.handle(&key(KeyCode::Char('q')), &inventory);
    assert!(view.editing);
    assert_eq!(
        view.handle(&key(KeyCode::Esc), &inventory),
        tui::Action::Continue
    );
    assert_eq!(
        view.handle(&key(KeyCode::Esc), &inventory),
        tui::Action::Quit
    );
}
