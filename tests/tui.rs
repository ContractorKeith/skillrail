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
    assert!(text.contains("No indexed items."));
    assert!(text.contains("Discovery is not implemented in M0."));
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
