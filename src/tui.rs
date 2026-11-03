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
        if !inventory.items.is_empty() && selected >= inventory.items.len() {
            "No items match the current search/filters.\n".into()
        } else {
            crate::presentation::details(inventory, selected)
        }
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
    Toggle(bool),
    History,
    Install,
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
            KeyCode::Char('d') => {
                self.query.drift_only = !self.query.drift_only;
                true
            }
            KeyCode::Char('s') => {
                cycle(
                    &mut self.query.scope,
                    &["user", "project", "local", "unknown"],
                );
                true
            }
            KeyCode::Char('e') => return Action::Toggle(true),
            KeyCode::Char('b') => return Action::Toggle(false),
            KeyCode::Char('h') => return Action::History,
            KeyCode::Char('i') => return Action::Install,
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
            "Search: {}{} | kind: {} | agent: {} | scope: {}\n/: search  Enter: details  k/a/s: filters  d: drift  x: clear\nMatches: {} | drift: {} | e: enable  b: disable  h: history  i: install\n",
            crate::presentation::safe(self.query.query.as_deref().unwrap_or("")),
            if self.editing { "_" } else { "" },
            self.query.kind.as_deref().unwrap_or("all"),
            self.query.agent.as_deref().unwrap_or("all"),
            self.query.scope.as_deref().unwrap_or("all"),
            indices.len(),
            if self.query.drift_only {
                "drifted/unknown"
            } else {
                "all"
            }
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
    let mut mutations = MutationUi::default();
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
            .draw(|frame| {
                if mutations.active() {
                    mutations.render(frame)
                } else {
                    view.render(frame, &inventory)
                }
            })
            .map_err(|_| io_error())?;
        let event = event::read().map_err(|_| io_error())?;
        if let Event::Key(key) = &event
            && key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return Ok(());
        }
        if mutations.handle(&event, application, &mut inventory) {
            continue;
        }
        match view.handle(&event, &inventory) {
            Action::Quit => return Ok(()),
            Action::Refresh => {
                inventory = application.scan()?;
                view.refreshed(&inventory);
            }
            Action::Continue => {}
            Action::Toggle(enabled) => mutations.toggle(enabled, &view, &inventory),
            Action::History => mutations.history(application),
            Action::Install => mutations.install(),
        }
        // Resize and other events redraw on the next iteration.
    }
}

/// Mutation screens retain the actual private plan between preview and confirmation.
#[derive(Default)]
pub struct MutationUi {
    screen: MutationScreen,
    selected: usize,
    scroll: u16,
}
#[derive(Default)]
enum MutationScreen {
    #[default]
    Closed,
    Install(InstallForm),
    Picker(Vec<(crate::mutation::ToggleRequest, String)>),
    Preview(Box<crate::mutation::Plan>),
    History(crate::mutation::History),
    Message(String),
}
struct InstallForm {
    fields: [String; 6],
    selected: usize,
}
impl Default for InstallForm {
    fn default() -> Self {
        Self {
            fields: [
                String::new(),
                "local".into(),
                String::new(),
                String::new(),
                "codex".into(),
                "user".into(),
            ],
            selected: 0,
        }
    }
}
impl InstallForm {
    fn request(&self) -> crate::mutation::InstallRequest {
        crate::mutation::InstallRequest {
            source: self.fields[0].clone(),
            git: self.fields[1] == "git",
            revision: (!self.fields[2].is_empty()).then(|| self.fields[2].clone()),
            skill: (!self.fields[3].is_empty()).then(|| self.fields[3].clone()),
            agents: self.fields[4].split(',').map(str::to_owned).collect(),
            scope: self.fields[5].clone(),
        }
    }
}
impl MutationUi {
    pub fn install(&mut self) {
        self.screen = MutationScreen::Install(InstallForm::default());
        self.scroll = 0;
    }
    pub fn active(&self) -> bool {
        !matches!(self.screen, MutationScreen::Closed)
    }
    pub fn toggle(&mut self, enabled: bool, view: &View, inventory: &Inventory) {
        let item = view
            .query
            .indices(inventory)
            .get(view.selection.item)
            .copied();
        let Some(item) = item else {
            self.screen = MutationScreen::Message("No selected item.".into());
            return;
        };
        let choices = inventory
            .occurrences
            .iter()
            .filter(|o| o.item_id == inventory.items[item].id)
            .map(|o| {
                let agent = match o.agent {
                    crate::discovery::Agent::Claude => "claude",
                    crate::discovery::Agent::Codex => "codex",
                };
                let scope = match o.scope {
                    crate::discovery::Scope::User => "user",
                    crate::discovery::Scope::Project => "project",
                    crate::discovery::Scope::Local => "local",
                    crate::discovery::Scope::Unknown => "unknown",
                };
                (
                    crate::mutation::ToggleRequest {
                        id: o.id.clone(),
                        agent: agent.into(),
                        scope: scope.into(),
                        enabled,
                    },
                    format!(
                        "{} | {} | {} | {}",
                        agent,
                        scope,
                        crate::presentation::safe(&o.original_path.to_string_lossy()),
                        o.id
                    ),
                )
            })
            .collect();
        self.screen = MutationScreen::Picker(choices);
        self.selected = 0;
        self.scroll = 0;
    }
    pub fn history(&mut self, application: &Application) {
        self.screen = match application.operations() {
            Ok(h) => MutationScreen::History(h),
            Err(e) => MutationScreen::Message(e.to_string()),
        };
        self.selected = 0;
        self.scroll = 0;
    }
    /// Returns true when a modal screen consumed this event.
    pub fn handle(
        &mut self,
        event: &Event,
        application: &Application,
        inventory: &mut Inventory,
    ) -> bool {
        if !self.active() {
            return false;
        }
        let Event::Key(key) = event else {
            return true;
        };
        if key.kind != KeyEventKind::Press {
            return true;
        }
        if let MutationScreen::Install(form) = &mut self.screen {
            match key.code {
                KeyCode::Esc => self.screen = MutationScreen::Closed,
                KeyCode::BackTab | KeyCode::Up => form.selected = form.selected.saturating_sub(1),
                KeyCode::Tab | KeyCode::Down => form.selected = (form.selected + 1).min(5),
                KeyCode::Backspace => {
                    form.fields[form.selected].pop();
                }
                KeyCode::Char(c) if !c.is_control() => {
                    if form.fields[form.selected].len() + c.len_utf8() <= 4096 {
                        form.fields[form.selected].push(c);
                    }
                }
                KeyCode::Enter if form.selected < 5 => form.selected += 1,
                KeyCode::Enter => {
                    if !["local", "git"].contains(&form.fields[1].as_str()) {
                        self.screen =
                            MutationScreen::Message("Source kind must be local or git.".into());
                    } else {
                        self.screen = match application.plan_install(form.request()) {
                            Ok(plan) => MutationScreen::Preview(Box::new(plan)),
                            Err(e) => MutationScreen::Message(e.to_string()),
                        };
                    }
                }
                _ => {}
            }
            return true;
        }
        if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
            self.screen = MutationScreen::Closed;
            return true;
        }
        match key.code {
            KeyCode::Down => {
                self.selected = self.selected.saturating_add(1);
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(5),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::Enter => {
                let screen = std::mem::take(&mut self.screen);
                self.screen = match screen {
                    MutationScreen::Picker(choices) => {
                        match choices.get(self.selected.min(choices.len().saturating_sub(1))) {
                            Some((request, _)) => match application.plan_toggle(request.clone()) {
                                Ok(plan) => MutationScreen::Preview(Box::new(plan)),
                                Err(e) => MutationScreen::Message(e.to_string()),
                            },
                            None => MutationScreen::Message("No occurrence selected.".into()),
                        }
                    }
                    MutationScreen::Preview(plan) => match application.apply(&plan) {
                        Ok(result) => match application.scan() {
                            Ok(fresh) => {
                                *inventory = fresh;
                                MutationScreen::Message(format!(
                                    "Operation {}: {}. h: history after closing this message.",
                                    result.operation_id, result.status
                                ))
                            }
                            Err(e) => MutationScreen::Message(format!(
                                "Operation applied; inventory refresh failed: {e}. Inspect history before retrying."
                            )),
                        },
                        Err(e) => MutationScreen::Message(e.to_string()),
                    },
                    other => other,
                };
                self.scroll = 0;
            }
            KeyCode::Char('u') => {
                if let MutationScreen::History(h) = &self.screen
                    && let Some(op) = h
                        .operations
                        .get(self.selected.min(h.operations.len().saturating_sub(1)))
                {
                    self.screen = match application.plan_undo(&op.operation_id) {
                        Ok(plan) => MutationScreen::Preview(Box::new(plan)),
                        Err(e) => MutationScreen::Message(e.to_string()),
                    };
                    self.scroll = 0;
                }
            }
            _ => {}
        }
        true
    }
    pub fn render(&self, frame: &mut Frame<'_>) {
        let text = match &self.screen {
            MutationScreen::Closed => String::new(),
            MutationScreen::Install(form) => {
                let labels = [
                    "Source (path or HTTPS; input hidden)",
                    "Kind: local|git",
                    "Git ref (blank = HEAD)",
                    "Skill subpath (blank = sole skill)",
                    "Agents: claude,codex",
                    "Scope: user|project",
                ];
                let mut text = String::from(
                    "Install snapshot | Enter/Tab: next | Up: previous | Backspace: edit | Esc: cancel\nOn Scope: Enter previews; a separate confirmation applies.\n",
                );
                for (n, label) in labels.iter().enumerate() {
                    let value = if n == 0 && !form.fields[n].is_empty() {
                        "<source entered>"
                    } else {
                        &form.fields[n]
                    };
                    text.push_str(&format!(
                        "{} {}: {}\n",
                        if form.selected == n { ">" } else { " " },
                        label,
                        value
                    ));
                }
                text
            }
            MutationScreen::Picker(choices) => {
                let selected = self.selected.min(choices.len().saturating_sub(1));
                let mut text = String::from(
                    "Select exact occurrence | arrows: select | Enter: preview | Esc: cancel\n",
                );
                for (i, (_, description)) in choices.iter().enumerate() {
                    text.push_str(&format!(
                        "{} {}\n",
                        if i == selected { ">" } else { " " },
                        description
                    ));
                }
                text
            }
            MutationScreen::Preview(plan) => format!(
                "Preview | Enter: apply | Esc: cancel | PgUp/PgDn: scroll\n{}",
                serde_json::to_string_pretty(plan.view())
                    .unwrap_or_else(|_| "Cannot encode safe preview.".into())
            ),
            MutationScreen::History(history) => {
                let selected = self
                    .selected
                    .min(history.operations.len().saturating_sub(1));
                let mut text = String::from(
                    "Operation history | arrows: select | u: preview undo | Esc: close\n",
                );
                for (i, op) in history.operations.iter().enumerate() {
                    text.push_str(&format!(
                        "{} {} | {} | {} files\n{}\n",
                        if i == selected { ">" } else { " " },
                        op.operation_id,
                        op.status,
                        op.changed_files,
                        op.recovery
                    ));
                }
                if history.operations.is_empty() {
                    text.push_str("No operations.\n");
                }
                text
            }
            MutationScreen::Message(text) => format!("{text}\nEsc: close"),
        };
        frame.render_widget(
            Paragraph::new(
                text.split('\n')
                    .map(crate::presentation::safe)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .block(Block::bordered().title("Skillrail operations"))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0)),
            frame.area(),
        );
    }
}
