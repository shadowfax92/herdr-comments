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
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use unicode_width::UnicodeWidthChar;

use crate::model::PaneSnapshot;
use crate::snapshot::rows;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourcePosition {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WrappedRow {
    source_row: usize,
    start_column: usize,
    end_column: usize,
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
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationState {
    snapshot: PaneSnapshot,
    lines: Vec<String>,
    cursor: SourcePosition,
    top: usize,
    viewport_height: usize,
    viewport_width: usize,
    mode: Mode,
    close_after_comment: bool,
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
            viewport_height: 1,
            viewport_width: 0,
            mode: Mode::Normal,
            close_after_comment: false,
            comment_count,
            error: None,
        }
    }

    pub fn new_inline(snapshot: PaneSnapshot, comment_count: usize) -> Self {
        let source = snapshot.text.clone();
        let mut state = Self::new(snapshot, comment_count);
        state.mode = Mode::Comment {
            source,
            editor: NoteEditor::default(),
        };
        state.close_after_comment = true;
        state
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

    fn exits_after_save(&self) -> bool {
        self.close_after_comment
    }

    pub fn set_viewport(&mut self, height: usize, width: usize) {
        self.viewport_height = height.max(1);
        let width = width.max(1);
        if self.viewport_width != width {
            let top = if self.viewport_width == 0 {
                SourcePosition {
                    row: self.top,
                    column: 0,
                }
            } else {
                wrapped_rows(&self.lines, self.viewport_width)
                    .get(self.top)
                    .map(|row| SourcePosition {
                        row: row.source_row,
                        column: row.start_column,
                    })
                    .unwrap_or(SourcePosition { row: 0, column: 0 })
            };
            self.viewport_width = width;
            self.top = wrapped_row_index(&wrapped_rows(&self.lines, width), top).unwrap_or(0);
        }
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
                if self.close_after_comment {
                    return InputOutcome::Exit;
                }
                self.mode = Mode::Normal;
                InputOutcome::Continue
            }
            (KeyCode::Enter, modifiers) if !modifiers.contains(KeyModifiers::ALT) => {
                InputOutcome::SaveComment {
                    source: source.clone(),
                    note: editor.text(),
                }
            }
            (KeyCode::Enter, modifiers) if modifiers.contains(KeyModifiers::ALT) => {
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
        if self.viewport_width == 0 {
            return;
        }
        let rows = wrapped_rows(&self.lines, self.viewport_width);
        let Some(cursor_row) = wrapped_row_index(&rows, self.cursor) else {
            return;
        };
        if cursor_row < self.top {
            self.top = cursor_row;
        } else if cursor_row >= self.top.saturating_add(self.viewport_height) {
            self.top = cursor_row
                .saturating_add(1)
                .saturating_sub(self.viewport_height);
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

pub fn run_annotation(store: &Store, run_id: &str) -> Result<()> {
    run_annotation_mode(store, run_id, false)
}

pub fn run_inline_annotation(store: &Store, run_id: &str) -> Result<()> {
    run_annotation_mode(store, run_id, true)
}

fn run_annotation_mode(store: &Store, run_id: &str, inline: bool) -> Result<()> {
    let run = store.load_annotation(run_id)?;
    let count = store.list_comments(&run.scope)?.len();
    let mut session = TerminalSession::start()?;
    let mut state = if inline {
        AnnotationState::new_inline(run.snapshot.clone(), count)
    } else {
        AnnotationState::new(run.snapshot.clone(), count)
    };

    loop {
        session.terminal.draw(|frame| render(frame, &mut state))?;
        match event::read().context("failed to read annotation input")? {
            Event::Key(key) => match state.handle_key(key) {
                InputOutcome::Continue => {}
                InputOutcome::SaveComment { source, note } => {
                    match store.add_comment(&run.scope, &source, &note) {
                        Ok(_) if state.exits_after_save() => {
                            store.delete_annotation(&run.id)?;
                            return Ok(());
                        }
                        Ok(_) => state.comment_saved(store.list_comments(&run.scope)?.len()),
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
        Layout::vertical([Constraint::Min(1), Constraint::Length(4)]).areas(frame.area());
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
    let rows = wrapped_rows(&state.lines, state.viewport_width);
    let visible = rows
        .iter()
        .skip(state.top)
        .take(usize::from(source_area.height))
        .map(|row| {
            styled.lines.get(row.source_row).map_or_else(
                || {
                    Line::raw(char_slice(
                        &state.lines[row.source_row],
                        row.start_column,
                        row.end_column,
                    ))
                },
                |line| styled_line_slice(line, row.start_column, row.end_column),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(visible)), source_area);
    render_selection(frame, source_area, state, &rows);

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
        " v/V select · c comment · esc cancel "
    } else {
        " hjkl scroll · v/V select · q done "
    };
    let help_style = if state.error().is_some() {
        Style::default().fg(Color::White).bg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(status).style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(state.error().unwrap_or(help)).style(help_style),
            Line::from(" after q: alt-shift-c final edit ")
                .style(Style::default().fg(Color::DarkGray)),
            Line::from("          alt-shift-p paste ").style(Style::default().fg(Color::DarkGray)),
        ]),
        footer_area,
    );

    if let Some(cursor_row) = wrapped_row_index(&rows, state.cursor) {
        let row = rows[cursor_row];
        let line = &state.lines[state.cursor.row];
        let cursor_column = display_column(line, state.cursor.column)
            .saturating_sub(display_column(line, row.start_column));
        let x = source_area
            .x
            .saturating_add(u16::try_from(cursor_column).unwrap_or(u16::MAX));
        let y = source_area.y.saturating_add(
            u16::try_from(cursor_row.saturating_sub(state.top)).unwrap_or(u16::MAX),
        );
        if x < source_area.right() && y < source_area.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
}

fn render_selection(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AnnotationState,
    rows: &[WrappedRow],
) {
    let style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let buffer = frame.buffer_mut();
    for (screen_row, row) in rows
        .iter()
        .skip(state.top)
        .take(usize::from(area.height))
        .enumerate()
    {
        let Some((from, to)) = state.selected_columns(row.source_row) else {
            continue;
        };
        let Some(line) = state.lines.get(row.source_row) else {
            continue;
        };
        let from = from.max(row.start_column);
        let to = to.min(row.end_column);
        if from >= to && row.start_column != row.end_column {
            continue;
        }
        let row_start = display_column(line, row.start_column);
        let start = display_column(line, from).saturating_sub(row_start);
        let end = display_column(line, to)
            .saturating_sub(row_start)
            .max(start.saturating_add(1));
        for column in start..end {
            let x = area
                .x
                .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
            let y = area
                .y
                .saturating_add(u16::try_from(screen_row).unwrap_or(u16::MAX));
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
    let rows = wrapped_editor_rows(&editor.lines, usize::from(inner.width));
    let cursor = SourcePosition {
        row: editor.row,
        column: editor.column,
    };
    let cursor_row = wrapped_row_index(&rows, cursor)
        .or_else(|| rows.iter().rposition(|row| row.source_row == editor.row))
        .unwrap_or(0);
    let top = cursor_row.saturating_sub(visible_rows.saturating_sub(1));
    let text = rows
        .iter()
        .skip(top)
        .take(visible_rows)
        .map(|row| {
            Line::from(char_slice(
                &editor.lines[row.source_row],
                row.start_column,
                row.end_column,
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(text), inner);

    let row = rows[cursor_row];
    let cursor_width = display_column(&editor.lines[editor.row], editor.column)
        .saturating_sub(display_column(&editor.lines[editor.row], row.start_column));
    let x = inner
        .x
        .saturating_add(u16::try_from(cursor_width).unwrap_or(u16::MAX));
    let y = inner
        .y
        .saturating_add(u16::try_from(cursor_row.saturating_sub(top)).unwrap_or(u16::MAX));
    if x < inner.right() && y < inner.bottom() {
        frame.set_cursor_position((x, y));
    }

    let help = if state.exits_after_save() {
        " type comment · enter collect & close · alt-enter newline · esc cancel & close "
    } else {
        " type comment · enter collect · alt-enter newline · esc back to NORMAL "
    };
    let message = state.error().unwrap_or(help);
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

fn wrapped_rows(lines: &[String], width: usize) -> Vec<WrappedRow> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for (source_row, line) in lines.iter().enumerate() {
        let mut start_column = 0;
        let mut row_width = 0usize;
        let mut end_column = 0;
        for (column, ch) in line.chars().enumerate() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if row_width > 0 && row_width.saturating_add(char_width) > width {
                rows.push(WrappedRow {
                    source_row,
                    start_column,
                    end_column: column,
                });
                start_column = column;
                row_width = 0;
            }
            row_width = row_width.saturating_add(char_width);
            end_column = column + 1;
        }
        rows.push(WrappedRow {
            source_row,
            start_column,
            end_column,
        });
    }
    rows
}

fn wrapped_row_index(rows: &[WrappedRow], position: SourcePosition) -> Option<usize> {
    rows.iter().position(|row| {
        row.source_row == position.row
            && (row.start_column == row.end_column && position.column == row.start_column
                || row.start_column <= position.column && position.column < row.end_column)
    })
}

fn wrapped_editor_rows(lines: &[String], width: usize) -> Vec<WrappedRow> {
    let width = width.max(1);
    let mut rows = wrapped_rows(lines, width);
    for (source_row, line) in lines.iter().enumerate().rev() {
        let Some(last) = rows.iter().rposition(|row| row.source_row == source_row) else {
            continue;
        };
        let row = rows[last];
        let row_width = display_column(line, row.end_column)
            .saturating_sub(display_column(line, row.start_column));
        if row_width >= width {
            rows.insert(
                last + 1,
                WrappedRow {
                    source_row,
                    start_column: row.end_column,
                    end_column: row.end_column,
                },
            );
        }
    }
    rows
}

fn styled_line_slice(line: &Line<'_>, from: usize, to: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut span_start = 0;
    for span in &line.spans {
        let span_len = span.content.chars().count();
        let span_end = span_start + span_len;
        let overlap_start = from.max(span_start);
        let overlap_end = to.min(span_end);
        if overlap_start < overlap_end {
            spans.push(Span::styled(
                char_slice(
                    span.content.as_ref(),
                    overlap_start - span_start,
                    overlap_end - span_start,
                ),
                span.style,
            ));
        }
        span_start = span_end;
    }
    Line {
        style: line.style,
        alignment: line.alignment,
        spans,
    }
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
        state.handle_paste("first line");
        state.handle_key(key(KeyCode::Enter, KeyModifiers::ALT));
        state.handle_paste("second line");
        assert_eq!(
            state.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            InputOutcome::SaveComment {
                source: "alpha\nbeta value".into(),
                note: "first line\nsecond line".into(),
            }
        );
    }

    #[test]
    fn inline_comment_starts_in_the_editor_and_closes_after_save_or_cancel() {
        let mut saved = AnnotationState::new_inline(snapshot(), 2);

        assert!(saved.is_commenting());
        saved.handle_paste("direct note");
        assert_eq!(
            saved.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            InputOutcome::SaveComment {
                source: "alpha\nbeta value\ngamma".into(),
                note: "direct note".into(),
            }
        );
        assert!(saved.exits_after_save());

        let mut cancelled = AnnotationState::new_inline(snapshot(), 2);
        assert_eq!(
            cancelled.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            InputOutcome::Exit
        );
    }

    #[test]
    fn comment_editor_wraps_typed_text_and_keeps_cursor_visible() {
        let mut state = AnnotationState::new_inline(snapshot(), 0);
        state.handle_paste("abcdefghij");
        let area = Rect::new(0, 0, 10, 12);
        let [_, note_area, _] = Layout::vertical([
            Constraint::Percentage(35),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .areas(area);
        let inner = Block::default().borders(Borders::ALL).inner(note_area);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();

        terminal.draw(|frame| render(frame, &mut state)).unwrap();

        let backend = terminal.backend();
        let row = |y| {
            (inner.x..inner.right())
                .map(|x| backend.buffer()[(x, y)].symbol())
                .collect::<String>()
        };
        assert_eq!(row(inner.y), "abcdefgh");
        assert_eq!(row(inner.y + 1), "ij      ");
        assert!(backend.cursor_visible());
        assert_eq!(backend.cursor_position(), (inner.x + 2, inner.y + 1).into());
    }

    #[test]
    fn snapshot_comment_escape_returns_to_the_snapshot() {
        let mut state = AnnotationState::new(snapshot(), 0);
        state.handle_key(key(KeyCode::Char('V'), KeyModifiers::SHIFT));
        state.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));

        assert_eq!(
            state.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            InputOutcome::Continue
        );
        assert!(!state.is_commenting());
    }

    #[test]
    fn comment_help_matches_inline_and_snapshot_exit_behavior() {
        let render_content = |mut state| {
            let mut terminal = Terminal::new(TestBackend::new(90, 12)).unwrap();
            terminal.draw(|frame| render(frame, &mut state)).unwrap();
            let buffer = terminal.backend().buffer();
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let inline = render_content(AnnotationState::new_inline(snapshot(), 0));
        assert!(inline.contains("type comment"));
        assert!(inline.contains("enter collect & close"));
        assert!(inline.contains("esc cancel & close"));

        let mut snapshot = AnnotationState::new(snapshot(), 0);
        snapshot.handle_key(key(KeyCode::Char('V'), KeyModifiers::SHIFT));
        snapshot.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));
        let snapshot = render_content(snapshot);
        assert!(snapshot.contains("enter collect"));
        assert!(snapshot.contains("esc back to NORMAL"));
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
    fn normal_mode_exit_closes_the_popup() {
        let mut state = AnnotationState::new(snapshot(), 2);
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
        assert!(content.contains("alt-shift-c final edit"));
        assert!(content.contains("alt-shift-p paste"));
    }

    #[test]
    fn wrapped_source_keeps_cursor_and_visual_selection_aligned() {
        let snapshot = PaneSnapshot {
            text: "abcdefghij".into(),
            ansi: "\u{1b}[31mabcdefghij\u{1b}[0m".into(),
            initial_top: 0,
            viewport_rows: 1,
            history_limited: false,
        };
        let mut state = AnnotationState::new(snapshot, 0);
        state.handle_key(key(KeyCode::Char('$'), KeyModifiers::NONE));
        state.handle_key(key(KeyCode::Char('v'), KeyModifiers::NONE));
        state.handle_key(key(KeyCode::Char('h'), KeyModifiers::NONE));
        state.handle_key(key(KeyCode::Char('h'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(5, 7)).unwrap();

        terminal.draw(|frame| render(frame, &mut state)).unwrap();

        let backend = terminal.backend();
        let row = |y| {
            (0..backend.buffer().area.width)
                .map(|x| backend.buffer()[(x, y)].symbol())
                .collect::<String>()
        };
        assert_eq!(row(0), "abcde");
        assert_eq!(row(1), "fghij");
        assert_eq!(backend.cursor_position(), (2, 1).into());
        assert_eq!(backend.buffer()[(0, 0)].fg, Color::Red);
        assert_eq!(backend.buffer()[(0, 1)].fg, Color::Red);
        assert_eq!(backend.buffer()[(2, 1)].bg, Color::Yellow);
        assert_eq!(backend.buffer()[(4, 1)].bg, Color::Yellow);
    }
}
