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
    render_view(frame, inventory, selected, scroll, "");
}
fn render_view(
    frame: &mut Frame<'_>,
    inventory: &Inventory,
    selected: usize,
    scroll: u16,
    header: &str,
) {
    let mut text = format!(
        "{header}Skills/MCP: {} | partial coverage | Last scan: {} (Unix ms)\nItem {}/{} | arrows: select | PgUp/PgDn: scroll | r: refresh | q: quit\n\n{}",
        inventory.items.len(),
        inventory.scanned_at_unix_ms,
        if selected >= inventory.items.len() {
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
        text.push_str(&format!(
            "\n{}",
            crate::presentation::diagnostic(diagnostic)
        ));
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

/// Interactive state only filters the last explicit scan; it never discovers sources.
#[derive(Default)]
pub struct View {
    pub selection: Selection,
    pub query: crate::query::Query,
    pub editing: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Continue,
    Refresh,
    Quit,
}
impl View {
    pub fn handle(&mut self, event: &Event, inventory: &Inventory) -> Action {
        let Event::Key(key) = event else {
            return Action::Continue;
        };
        if key.kind != KeyEventKind::Press {
            return Action::Continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }
        if self.editing {
            match key.code {
                KeyCode::Esc => {
                    self.query.query = None;
                    self.editing = false;
                }
                KeyCode::Enter => self.editing = false,
                KeyCode::Backspace => {
                    if let Some(query) = &mut self.query.query {
                        query.pop();
                    }
                }
                KeyCode::Char(c) if !c.is_control() => {
                    let query = self.query.query.get_or_insert_with(String::new);
                    if query.len() + c.len_utf8() <= 256 {
                        query.push(c);
                    }
                }
                _ => {}
            }
            self.selection = Selection::default();
            return Action::Continue;
        }
        if should_quit(event) {
            return Action::Quit;
        }
        let changed = match key.code {
            KeyCode::Char('/') => {
                self.editing = true;
                true
            }
            KeyCode::Char('a') => {
                cycle(&mut self.query.agent, &["claude", "codex"]);
                true
            }
            KeyCode::Char('k') => {
                cycle(&mut self.query.kind, &["skill", "mcp"]);
                true
            }
            KeyCode::Char('s') => {
                cycle(
                    &mut self.query.scope,
                    &["user", "project", "local", "unknown"],
                );
                true
            }
            KeyCode::Char('r') => return Action::Refresh,
            KeyCode::Char('x') => {
                self.query = crate::query::Query::default();
                true
            }
            _ => false,
        };
        if changed {
            self.selection = Selection::default();
        }
        self.selection
            .handle(event, self.query.indices(inventory).len());
        Action::Continue
    }
    pub fn render(&self, frame: &mut Frame<'_>, inventory: &Inventory) {
        let indices = self.query.indices(inventory);
        let selected = indices
            .get(self.selection.item)
            .copied()
            .unwrap_or(usize::MAX);
        let header = format!(
            "Search: {}{} | kind: {} | agent: {} | scope: {}\n/: search  Enter: details  k/a/s: filters  x: clear\nMatches: {}\n",
            crate::presentation::safe(self.query.query.as_deref().unwrap_or("")),
            if self.editing { "_" } else { "" },
            self.query.kind.as_deref().unwrap_or("all"),
            self.query.agent.as_deref().unwrap_or("all"),
            self.query.scope.as_deref().unwrap_or("all"),
            indices.len()
        );
        render_view(frame, inventory, selected, self.selection.scroll, &header);
    }
    pub fn refreshed(&mut self, inventory: &Inventory) {
        self.selection.item = self
            .selection
            .item
            .min(self.query.indices(inventory).len().saturating_sub(1));
        self.selection.scroll = 0;
    }
}
fn cycle(value: &mut Option<String>, choices: &[&str]) {
    *value = match value.as_deref() {
        None => Some(choices[0].into()),
        Some(current) => choices
            .iter()
            .position(|v| *v == current)
            .and_then(|index| choices.get(index + 1))
            .map(|v| (*v).into()),
    };
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
    let mut view = View::default();
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
            .draw(|frame| view.render(frame, &inventory))
            .map_err(|_| io_error())?;
        let event = event::read().map_err(|_| io_error())?;
        match view.handle(&event, &inventory) {
            Action::Quit => return Ok(()),
            Action::Refresh => {
                inventory = application.scan()?;
                view.refreshed(&inventory);
            }
            Action::Continue => {}
        }
        // Resize and other events redraw on the next iteration.
    }
}
