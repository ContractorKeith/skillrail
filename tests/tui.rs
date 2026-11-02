use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use skillrail::{Application, Paths, tui};

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
    assert!(text.contains("No standalone skills found"));
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
