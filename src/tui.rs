//! Minimal real terminal UI; rendering and events are testable without a terminal.
use crate::{AppError, Application, Inventory};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    widgets::{Block, Paragraph, Wrap},
};
use std::io::IsTerminal;

pub fn require_terminal() -> Result<(), AppError> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(AppError::new(
            "terminal_required",
            "TUI requires terminal stdin and stdout; use list --json for piped output.",
        ));
    }
    Ok(())
}

pub fn should_quit(event: &Event) -> bool {
    matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press
        && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))))
}

pub fn render(frame: &mut Frame<'_>, inventory: &Inventory) {
    let text = format!(
        "No indexed items.\nDiscovery is not implemented in M0.\nEmpty inventory does not establish absence of skills or MCP entries.\n\nLast scan: {} (Unix ms)\n\nq / Esc / Ctrl-C: quit | r: refresh",
        inventory.scanned_at_unix_ms
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title("Skillrail"))
            .wrap(Wrap { trim: true }),
        frame.area(),
    );
}

// Drop also runs on I/O errors and unwinding; Ratatui installs its panic restore hook.
struct RestoreTerminal;
impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

pub fn run(application: &Application) -> Result<(), AppError> {
    require_terminal()?;
    let mut inventory = application.scan()?;
    let io_error = || {
        AppError::new(
            "terminal_unavailable",
            "Terminal I/O failed; terminal restoration was attempted.",
        )
    };
    let _restore = RestoreTerminal;
    let mut terminal = ratatui::try_init().map_err(|_| io_error())?;
    loop {
        terminal
            .draw(|frame| render(frame, &inventory))
            .map_err(|_| io_error())?;
        let event = event::read().map_err(|_| io_error())?;
        if should_quit(&event) {
            return Ok(());
        }
        if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('r'))
        {
            inventory = application.scan()?;
        }
        // Resize and other events redraw on the next iteration.
    }
}
