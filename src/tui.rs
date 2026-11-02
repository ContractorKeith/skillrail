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
    render_selection(frame, inventory, 0, 0);
}

pub fn render_selection(
    frame: &mut Frame<'_>,
    inventory: &Inventory,
    selected: usize,
    scroll: u16,
) {
    let mut text = format!(
        "Standalone skills: {} | partial coverage | Last scan: {} (Unix ms)\nItem {}/{} | arrows: select | PgUp/PgDn: scroll | r: refresh | q: quit\n\n{}",
        inventory.items.len(),
        inventory.scanned_at_unix_ms,
        if inventory.items.is_empty() {
            0
        } else {
            selected + 1
        },
        inventory.items.len(),
        crate::presentation::details(inventory, selected)
    );
    text.push_str(&format!(
        "\nHome: {}\nProject: {}\nState: {}\n",
        crate::presentation::safe(&inventory.context.home.to_string_lossy()),
        crate::presentation::safe(&inventory.context.project.to_string_lossy()),
        crate::presentation::safe(&inventory.context.state_dir.to_string_lossy())
    ));
    for diagnostic in &inventory.diagnostics {
        text.push_str(&format!("\n{}: {}", diagnostic.code, diagnostic.message));
    }
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title("Skillrail"))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        frame.area(),
    );
}

#[derive(Default)]
pub struct Selection {
    pub item: usize,
    pub scroll: u16,
}
impl Selection {
    pub fn handle(&mut self, event: &Event, item_count: usize) {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Down => {
                    self.item = (self.item + 1).min(item_count.saturating_sub(1));
                    self.scroll = 0;
                }
                KeyCode::Up => {
                    self.item = self.item.saturating_sub(1);
                    self.scroll = 0;
                }
                KeyCode::PageDown => self.scroll = self.scroll.saturating_add(5),
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
                _ => {}
            }
        }
    }
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
    let mut selection = Selection::default();
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
            .draw(|frame| render_selection(frame, &inventory, selection.item, selection.scroll))
            .map_err(|_| io_error())?;
        let event = event::read().map_err(|_| io_error())?;
        if should_quit(&event) {
            return Ok(());
        }
        if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('r'))
        {
            inventory = application.scan()?;
            selection.item = selection.item.min(inventory.items.len().saturating_sub(1));
            selection.scroll = 0;
        }
        selection.handle(&event, inventory.items.len());
        // Resize and other events redraw on the next iteration.
    }
}
