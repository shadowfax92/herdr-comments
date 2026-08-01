use std::io::{self, Stdout};

use ansi_to_tui::IntoText;
use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::annotation::ReviewTarget;
use crate::herdr::HerdrClient;
use crate::model::PaneSnapshot;
use crate::review::{ReviewService, ReviewStart};
use crate::snapshot::rows;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourcePosition {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisualKind {
    Character,
    Line,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    Visual {
        anchor: SourcePosition,
        kind: VisualKind,
    },
    Comment {
        source: String,
        editor: NoteEditor,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputOutcome {
    Continue,
    SaveComment { source: String, note: String },
    Review,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationState {
    snapshot: PaneSnapshot,
    lines: Vec<String>,
    cursor: SourcePosition,
    top: usize,
    horizontal: usize,
    viewport_height: usize,
    viewport_width: usize,
    mode: Mode,
    comment_count: usize,
    error: Option<String>,
}

impl AnnotationState {
    pub fn new(snapshot: PaneSnapshot, comment_count: usize) -> Self {
        let lines = rows(&snapshot.text);
        let top = snapshot.initial_top.min(lines.len().saturating_sub(1));
        let cursor_row = top
            .saturating_add(snapshot.viewport_rows.saturating_sub(1))
            .min(lines.len().saturating_sub(1));
        Self {
            snapshot,
            lines,
            cursor: SourcePosition {
                row: cursor_row,
                column: 0,
            },
            top,
            horizontal: 0,
            viewport_height: 1,
            viewport_width: 1,
            mode: Mode::Normal,
            comment_count,
            error: None,
        }
    }

    pub fn cursor(&self) -> SourcePosition {
        self.cursor
    }

    pub fn top(&self) -> usize {
        self.top
    }

    pub fn comment_count(&self) -> usize {
        self.comment_count
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_commenting(&self) -> bool {
        matches!(self.mode, Mode::Comment { .. })
    }

    pub fn set_viewport(&mut self, height: usize, width: usize) {
        self.viewport_height = height.max(1);
        self.viewport_width = width.max(1);
        self.ensure_cursor_visible();
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    pub fn comment_saved(&mut self, count: usize) {
        self.comment_count = count;
        self.mode = Mode::Normal;
        self.error = None;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> InputOutcome {
        if !key.is_press() {
            return InputOutcome::Continue;
        }
        self.error = None;
        if matches!(self.mode, Mode::Comment { .. }) {
            return self.handle_comment_key(key);
        }
        self.handle_source_key(key)
    }

    pub fn handle_paste(&mut self, text: &str) {
        if let Mode::Comment { editor, .. } = &mut self.mode {
            editor.insert_text(text);
            self.error = None;
        }
    }

    fn handle_source_key(&mut self, key: KeyEvent) -> InputOutcome {
        if review_key(key) {
            return InputOutcome::Review;
        }
        match key.code {
            KeyCode::Esc => {
                if matches!(self.mode, Mode::Visual { .. }) {
                    self.mode = Mode::Normal;
                    return InputOutcome::Continue;
                }
                return InputOutcome::Exit;
            }
            KeyCode::Char('q') if key.modifiers.is_empty() => return InputOutcome::Exit,
            KeyCode::Char('v') if key.modifiers.is_empty() => {
                if matches!(self.mode, Mode::Visual { .. }) {
                    self.mode = Mode::Normal;
                } else {
                    self.mode = Mode::Visual {
                        anchor: self.cursor,
                        kind: VisualKind::Character,
                    };
                }
                return InputOutcome::Continue;
            }
            KeyCode::Char('V') | KeyCode::Char('v')
                if key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.mode = Mode::Visual {
                    anchor: self.cursor,
                    kind: VisualKind::Line,
                };
                return InputOutcome::Continue;
            }
            KeyCode::Char('c') if matches!(self.mode, Mode::Visual { .. }) => {
                let source = self.selected_text();
                if source.trim().is_empty() {
                    self.error = Some("Select non-empty text before commenting".into());
                    return InputOutcome::Continue;
                }
                self.mode = Mode::Comment {
                    source,
                    editor: NoteEditor::default(),
                };
                return InputOutcome::Continue;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_vertical(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_vertical(1),
            KeyCode::Left | KeyCode::Char('h') => self.move_left(),
            KeyCode::Right | KeyCode::Char('l') => self.move_right(),
            KeyCode::Home | KeyCode::Char('0') => self.cursor.column = 0,
            KeyCode::End | KeyCode::Char('$') => self.move_line_end(),
            KeyCode::PageUp => self.move_page(-1),
            KeyCode::PageDown => self.move_page(1),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_half_page(-1)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_half_page(1)
            }
            KeyCode::Char('g') if key.modifiers.is_empty() => self.move_to_row(0),
            KeyCode::Char('G') | KeyCode::Char('g')
                if key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.move_to_row(self.lines.len().saturating_sub(1))
            }
            _ => return InputOutcome::Continue,
        }
        self.ensure_cursor_visible();
        InputOutcome::Continue
    }

    fn handle_comment_key(&mut self, key: KeyEvent) -> InputOutcome {
        let Mode::Comment { source, editor } = &mut self.mode else {
            return InputOutcome::Continue;
        };
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.mode = Mode::Normal;
                InputOutcome::Continue
            }
            (KeyCode::Char('s' | 'S'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                InputOutcome::SaveComment {
                    source: source.clone(),
                    note: editor.text(),
                }
            }
            (KeyCode::Enter, _) => {
                editor.insert_newline();
                InputOutcome::Continue
            }
            (KeyCode::Left, _) => {
                editor.move_left();
                InputOutcome::Continue
            }
            (KeyCode::Right, _) => {
                editor.move_right();
                InputOutcome::Continue
            }
            (KeyCode::Up, _) => {
                editor.move_vertical(-1);
                InputOutcome::Continue
            }
            (KeyCode::Down, _) => {
                editor.move_vertical(1);
                InputOutcome::Continue
            }
            (KeyCode::Home, _) => {
                editor.column = 0;
                InputOutcome::Continue
            }
            (KeyCode::End, _) => {
                editor.column = editor.line_len();
                InputOutcome::Continue
            }
            (KeyCode::Backspace, _) => {
                editor.backspace();
                InputOutcome::Continue
            }
            (KeyCode::Delete, _) => {
                editor.delete();
                InputOutcome::Continue
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                editor.insert_char(ch);
                InputOutcome::Continue
            }
            _ => InputOutcome::Continue,
        }
    }

    fn move_vertical(&mut self, delta: isize) {
        let row = self.cursor.row.saturating_add_signed(delta);
        self.move_to_row(row.min(self.lines.len().saturating_sub(1)));
    }

    fn move_page(&mut self, direction: isize) {
        let distance = self.viewport_height.max(1);
        self.move_vertical(direction.saturating_mul(distance as isize));
    }

    fn move_half_page(&mut self, direction: isize) {
        let distance = (self.viewport_height / 2).max(1);
        self.move_vertical(direction.saturating_mul(distance as isize));
    }

    fn move_to_row(&mut self, row: usize) {
        self.cursor.row = row.min(self.lines.len().saturating_sub(1));
        self.cursor.column = self
            .cursor
            .column
            .min(self.max_source_column(self.cursor.row));
    }

    fn move_left(&mut self) {
        self.cursor.column = self.cursor.column.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor.column = self
            .cursor
            .column
            .saturating_add(1)
            .min(self.max_source_column(self.cursor.row));
    }

    fn move_line_end(&mut self) {
        self.cursor.column = self.max_source_column(self.cursor.row);
    }

    fn max_source_column(&self, row: usize) -> usize {
        self.lines
            .get(row)
            .map(|line| line.chars().count().saturating_sub(1))
            .unwrap_or(0)
    }

    fn ensure_cursor_visible(&mut self) {
        if self.cursor.row < self.top {
            self.top = self.cursor.row;
        } else if self.cursor.row >= self.top.saturating_add(self.viewport_height) {
            self.top = self
                .cursor
                .row
                .saturating_add(1)
                .saturating_sub(self.viewport_height);
        }
        let cursor_column = display_column(&self.lines[self.cursor.row], self.cursor.column);
        if cursor_column < self.horizontal {
            self.horizontal = cursor_column;
        } else if cursor_column >= self.horizontal.saturating_add(self.viewport_width) {
            self.horizontal = cursor_column
                .saturating_add(1)
                .saturating_sub(self.viewport_width);
        }
    }

    fn selected_text(&self) -> String {
        let Mode::Visual { anchor, kind } = self.mode else {
            return String::new();
        };
        let (start, end) = ordered(anchor, self.cursor);
        let mut selected = Vec::with_capacity(end.row.saturating_sub(start.row) + 1);
        for row in start.row..=end.row {
            let line = &self.lines[row];
            let char_count = line.chars().count();
            let (from, to) = match kind {
                VisualKind::Line => (0, char_count),
                VisualKind::Character if start.row == end.row => {
                    (start.column, end.column.saturating_add(1))
                }
                VisualKind::Character if row == start.row => (start.column, char_count),
                VisualKind::Character if row == end.row => (0, end.column.saturating_add(1)),
                VisualKind::Character => (0, char_count),
            };
            selected.push(char_slice(line, from, to));
        }
        selected.join("\n")
    }

    fn selected_columns(&self, row: usize) -> Option<(usize, usize)> {
        let Mode::Visual { anchor, kind } = self.mode else {
            return None;
        };
        let (start, end) = ordered(anchor, self.cursor);
        if row < start.row || row > end.row {
            return None;
        }
        let char_count = self.lines[row].chars().count();
        if char_count == 0 {
            return Some((0, 1));
        }
        match kind {
            VisualKind::Line => Some((0, char_count)),
            VisualKind::Character if start.row == end.row => {
                Some((start.column, end.column.saturating_add(1)))
            }
            VisualKind::Character if row == start.row => Some((start.column, char_count)),
            VisualKind::Character if row == end.row => Some((0, end.column.saturating_add(1))),
            VisualKind::Character => Some((0, char_count)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoteEditor {
    lines: Vec<String>,
    row: usize,
    column: usize,
}

impl Default for NoteEditor {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            column: 0,
        }
    }
}

impl NoteEditor {
    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn line_len(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    fn insert_char(&mut self, ch: char) {
        let byte = char_byte(&self.lines[self.row], self.column);
        self.lines[self.row].insert(byte, ch);
        self.column += 1;
    }

    fn insert_newline(&mut self) {
        let byte = char_byte(&self.lines[self.row], self.column);
        let tail = self.lines[self.row].split_off(byte);
        self.row += 1;
        self.column = 0;
        self.lines.insert(self.row, tail);
    }

    fn insert_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for ch in normalized.chars() {
            if ch == '\n' {
                self.insert_newline();
            } else if !ch.is_control() || ch == '\t' {
                self.insert_char(ch);
            }
        }
    }

    fn move_left(&mut self) {
        if self.column > 0 {
            self.column -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.column = self.line_len();
        }
    }

    fn move_right(&mut self) {
        if self.column < self.line_len() {
            self.column += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.column = 0;
        }
    }

    fn move_vertical(&mut self, delta: isize) {
        self.row = self
            .row
            .saturating_add_signed(delta)
            .min(self.lines.len().saturating_sub(1));
        self.column = self.column.min(self.line_len());
    }

    fn backspace(&mut self) {
        if self.column > 0 {
            let end = char_byte(&self.lines[self.row], self.column);
            let start = char_byte(&self.lines[self.row], self.column - 1);
            self.lines[self.row].drain(start..end);
            self.column -= 1;
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.column = self.line_len();
            self.lines[self.row].push_str(&current);
        }
    }

    fn delete(&mut self) {
        if self.column < self.line_len() {
            let start = char_byte(&self.lines[self.row], self.column);
            let end = char_byte(&self.lines[self.row], self.column + 1);
            self.lines[self.row].drain(start..end);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }
}

pub fn run_annotation<H: HerdrClient>(store: &Store, herdr: &H, run_id: &str) -> Result<()> {
    let run = store.load_annotation(run_id)?;
    let count = store.list_comments(&run.scope)?.len();
    let mut session = TerminalSession::start()?;
    let mut state = AnnotationState::new(run.snapshot.clone(), count);

    loop {
        session.terminal.draw(|frame| render(frame, &mut state))?;
        match event::read().context("failed to read annotation input")? {
            Event::Key(key) => match state.handle_key(key) {
                InputOutcome::Continue => {}
                InputOutcome::SaveComment { source, note } => {
                    match store.add_comment(&run.scope, &source, &note) {
                        Ok(_) => state.comment_saved(store.list_comments(&run.scope)?.len()),
                        Err(error) => state.set_error(error.to_string()),
                    }
                }
                InputOutcome::Review => {
                    let target = ReviewTarget {
                        pane_id: run.pane_id.clone(),
                        scope: run.scope.clone(),
                        annotation_run_id: Some(run.id.clone()),
                        overlay_pane_id: run.overlay_pane_id.clone(),
                    };
                    match ReviewService::new(store, herdr).start(&target) {
                        Ok(ReviewStart::Opened(_)) => {}
                        Ok(ReviewStart::Empty) => {
                            state.set_error("Collect at least one comment before review")
                        }
                        Err(error) => state.set_error(error.to_string()),
                    }
                }
                InputOutcome::Exit => {
                    store.delete_annotation(&run.id)?;
                    return Ok(());
                }
            },
            Event::Paste(text) => state.handle_paste(&text),
            _ => {}
        }
    }
}

pub fn render(frame: &mut Frame<'_>, state: &mut AnnotationState) {
    if state.is_commenting() {
        render_comment(frame, state);
    } else {
        render_source(frame, state);
    }
}

fn render_source(frame: &mut Frame<'_>, state: &mut AnnotationState) {
    let [source_area, footer_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(frame.area());
    state.set_viewport(
        usize::from(source_area.height),
        usize::from(source_area.width),
    );

    let styled = state
        .snapshot
        .ansi
        .as_bytes()
        .into_text()
        .unwrap_or_else(|_| Text::raw(state.snapshot.text.clone()));
    let visible = styled
        .lines
        .into_iter()
        .skip(state.top)
        .take(usize::from(source_area.height))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(visible))
            .scroll((0, u16::try_from(state.horizontal).unwrap_or(u16::MAX))),
        source_area,
    );
    render_selection(frame, source_area, state);

    let mode = if matches!(state.mode, Mode::Visual { .. }) {
        "VISUAL"
    } else {
        "NORMAL"
    };
    let limited = if state.snapshot.history_limited {
        " · history limited"
    } else {
        ""
    };
    let status = format!(
        " {mode} · {} comment{}{} ",
        state.comment_count,
        if state.comment_count == 1 { "" } else { "s" },
        limited
    );
    let help = if matches!(state.mode, Mode::Visual { .. }) {
        " v/V select · c comment · esc cancel · alt-shift-c review "
    } else {
        " hjkl scroll · v/V select · q close · alt-shift-c review "
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(status).style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(state.error().unwrap_or(help)).style(if state.error().is_some() {
                Style::default().fg(Color::White).bg(Color::Red)
            } else {
                Style::default().fg(Color::DarkGray)
            }),
        ]),
        footer_area,
    );

    let line = &state.lines[state.cursor.row];
    let cursor_column = display_column(line, state.cursor.column);
    if cursor_column >= state.horizontal {
        let x = source_area
            .x
            .saturating_add(u16::try_from(cursor_column - state.horizontal).unwrap_or(u16::MAX));
        let y = source_area.y.saturating_add(
            u16::try_from(state.cursor.row.saturating_sub(state.top)).unwrap_or(u16::MAX),
        );
        if x < source_area.right() && y < source_area.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
}

fn render_selection(frame: &mut Frame<'_>, area: Rect, state: &AnnotationState) {
    let style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let buffer = frame.buffer_mut();
    for row in state.top..state.top.saturating_add(usize::from(area.height)) {
        let Some((from, to)) = state.selected_columns(row) else {
            continue;
        };
        let Some(line) = state.lines.get(row) else {
            continue;
        };
        let start = display_column(line, from);
        let end = display_column(line, to).max(start.saturating_add(1));
        for column in start..end {
            if column < state.horizontal {
                continue;
            }
            let x = area
                .x
                .saturating_add(u16::try_from(column - state.horizontal).unwrap_or(u16::MAX));
            let y = area
                .y
                .saturating_add(u16::try_from(row.saturating_sub(state.top)).unwrap_or(u16::MAX));
            if x < area.right() && y < area.bottom() {
                buffer[(x, y)].set_style(style);
            }
        }
    }
}

fn render_comment(frame: &mut Frame<'_>, state: &AnnotationState) {
    let Mode::Comment { source, editor } = &state.mode else {
        return;
    };
    let [source_area, note_area, footer_area] = Layout::vertical([
        Constraint::Percentage(35),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .areas(frame.area());
    let quote = source
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(
        Paragraph::new(quote)
            .block(Block::default().borders(Borders::ALL).title(" Selection "))
            .wrap(Wrap { trim: false }),
        source_area,
    );

    let block = Block::default().borders(Borders::ALL).title(" Comment ");
    let inner = block.inner(note_area);
    frame.render_widget(block, note_area);
    let visible_rows = usize::from(inner.height).max(1);
    let top = editor.row.saturating_sub(visible_rows.saturating_sub(1));
    let text = editor
        .lines
        .iter()
        .skip(top)
        .take(visible_rows)
        .cloned()
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(text), inner);

    let cursor_width =
        UnicodeWidthStr::width(char_slice(&editor.lines[editor.row], 0, editor.column).as_str());
    let x = inner
        .x
        .saturating_add(u16::try_from(cursor_width).unwrap_or(u16::MAX));
    let y = inner
        .y
        .saturating_add(u16::try_from(editor.row.saturating_sub(top)).unwrap_or(u16::MAX));
    if x < inner.right() && y < inner.bottom() {
        frame.set_cursor_position((x, y));
    }

    let message = state
        .error()
        .unwrap_or(" ctrl-s collect · enter newline · esc cancel ");
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(" COMMENT · {} collected ", state.comment_count)).style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(message).style(if state.error().is_some() {
                Style::default().fg(Color::White).bg(Color::Red)
            } else {
                Style::default().fg(Color::DarkGray)
            }),
        ]),
        footer_area,
    );
}

fn review_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c' | 'C'))
        && key.modifiers.contains(KeyModifiers::ALT)
        && (key.modifiers.contains(KeyModifiers::SHIFT) || matches!(key.code, KeyCode::Char('C')))
}

fn ordered(first: SourcePosition, second: SourcePosition) -> (SourcePosition, SourcePosition) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn char_byte(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn char_slice(text: &str, from: usize, to: usize) -> String {
    let start = char_byte(text, from);
    let end = char_byte(text, to);
    text[start.min(end)..end].to_owned()
}

fn display_column(text: &str, column: usize) -> usize {
    text.chars()
        .take(column)
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn start() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste) {
            let _ = disable_raw_mode();
            return Err(error).context("failed to initialize annotation terminal");
        }
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEvent, KeyEventKind};
    use ratatui::backend::TestBackend;

    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press)
    }

    fn snapshot() -> PaneSnapshot {
        PaneSnapshot {
            text: "alpha\nbeta value\ngamma".into(),
            ansi: "\u{1b}[31malpha\u{1b}[0m\nbeta value\ngamma".into(),
            initial_top: 0,
            viewport_rows: 3,
            history_limited: false,
        }
    }

    #[test]
    fn visual_selection_opens_a_multiline_comment_editor() {
        let mut state = AnnotationState::new(snapshot(), 0);
        state.set_viewport(3, 40);
        state.handle_key(key(KeyCode::Char('k'), KeyModifiers::NONE));
        state.handle_key(key(KeyCode::Char('V'), KeyModifiers::SHIFT));
        state.handle_key(key(KeyCode::Char('k'), KeyModifiers::NONE));
        state.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));

        assert!(state.is_commenting());
        state.handle_paste("first line\nsecond line");
        assert_eq!(
            state.handle_key(key(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            InputOutcome::SaveComment {
                source: "alpha\nbeta value".into(),
                note: "first line\nsecond line".into(),
            }
        );
    }

    #[test]
    fn scrolling_is_clamped_and_keeps_the_cursor_visible() {
        let mut snapshot = snapshot();
        snapshot.text = (0..30)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        snapshot.ansi = snapshot.text.clone();
        snapshot.viewport_rows = 5;
        let mut state = AnnotationState::new(snapshot, 0);
        state.set_viewport(5, 20);

        state.handle_key(key(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(state.cursor().row, 0);
        assert_eq!(state.top(), 0);
        state.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(state.cursor().row, 2);
        state.handle_key(key(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(state.cursor().row, 29);
        assert_eq!(state.top(), 25);
    }

    #[test]
    fn review_chord_and_exit_are_distinct_outcomes() {
        let mut state = AnnotationState::new(snapshot(), 2);
        assert_eq!(
            state.handle_key(key(
                KeyCode::Char('C'),
                KeyModifiers::ALT | KeyModifiers::SHIFT
            )),
            InputOutcome::Review
        );
        assert_eq!(
            state.handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            InputOutcome::Exit
        );
    }

    #[test]
    fn ansi_source_and_status_render_together() {
        let mut state = AnnotationState::new(snapshot(), 2);
        let backend = TestBackend::new(48, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut state)).unwrap();
        let buffer = terminal.backend().buffer();
        let content = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(buffer[(0, 0)].fg, Color::Red);
        assert!(content.contains("NORMAL · 2 comments"));
        assert!(content.contains("v/V select"));
    }
}
