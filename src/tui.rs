use std::io::{self, Stdout};

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
use unicode_width::UnicodeWidthStr;

use crate::capture::{CaptureService, Completion};
use crate::format::preview_lines;
use crate::herdr::HerdrClient;
use crate::model::Draft;
use crate::store::Store;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EditorState {
    note: String,
    cursor: usize,
    error: Option<String>,
}

impl EditorState {
    pub fn note(&self) -> &str {
        &self.note
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Completion> {
        if !key.is_press() {
            return None;
        }
        self.error = None;
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => Some(Completion::Cancel),
            (KeyCode::Char('p' | 'P'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Completion::PasteAll)
            }
            (KeyCode::Enter, modifiers)
                if modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) =>
            {
                Some(Completion::Collect)
            }
            (KeyCode::Enter, _) => Some(Completion::Insert),
            (KeyCode::Left, _) => {
                self.move_left();
                None
            }
            (KeyCode::Right, _) => {
                self.move_right();
                None
            }
            (KeyCode::Home, _) => {
                self.cursor = 0;
                None
            }
            (KeyCode::End, _) => {
                self.cursor = self.note.len();
                None
            }
            (KeyCode::Backspace, _) => {
                self.backspace();
                None
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.note.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                None
            }
            _ => None,
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        self.error = None;
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        self.note.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }

    fn move_left(&mut self) {
        if let Some((index, _)) = self.note[..self.cursor].char_indices().next_back() {
            self.cursor = index;
        }
    }

    fn move_right(&mut self) {
        if let Some(ch) = self.note[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
    }

    fn backspace(&mut self) {
        let previous = self.note[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index);
        if let Some(previous) = previous {
            self.note.drain(previous..self.cursor);
            self.cursor = previous;
        }
    }
}

pub fn run_capture<H: HerdrClient>(store: &Store, herdr: &H, draft_id: &str) -> Result<()> {
    let draft = store.load_draft(draft_id)?;
    let collected_count = store.list_comments(&draft.scope)?.len();
    let mut session = TerminalSession::start()?;
    let mut editor = EditorState::default();
    let service = CaptureService::new(store, herdr);

    loop {
        session
            .terminal
            .draw(|frame| render(frame, &draft, collected_count, &editor))?;
        match event::read().context("failed to read popup input")? {
            Event::Key(key) => {
                let Some(completion) = editor.handle_key(key) else {
                    continue;
                };
                match service.complete(&draft.id, editor.note(), completion) {
                    Ok(_) => return Ok(()),
                    Err(error) => editor.set_error(error.to_string()),
                }
            }
            Event::Paste(text) => editor.handle_paste(&text),
            _ => {}
        }
    }
}

pub fn render(frame: &mut Frame<'_>, draft: &Draft, count: usize, editor: &EditorState) {
    let [preview_area, note_area, error_area, footer_area] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .areas(frame.area());

    let preview = Text::from(
        preview_lines(&draft.source_text, 4, 2_000)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>(),
    );
    frame.render_widget(
        Paragraph::new(preview)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Copied text · {count} collected ")),
            )
            .wrap(Wrap { trim: false }),
        preview_area,
    );

    frame.render_widget(
        Paragraph::new(editor.note.as_str())
            .block(Block::default().borders(Borders::ALL).title(" Comment ")),
        note_area,
    );

    if let Some(error) = editor.error() {
        frame.render_widget(
            Paragraph::new(error).style(Style::default().fg(Color::Red)),
            error_area,
        );
    }

    frame.render_widget(
        Paragraph::new(vec![
            Line::from("enter insert · option+enter collect"),
            Line::from("ctrl+p paste all · esc discard").style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]),
        footer_area,
    );

    set_editor_cursor(frame, note_area, editor);
}

fn set_editor_cursor(frame: &mut Frame<'_>, area: Rect, editor: &EditorState) {
    if area.width <= 2 || area.height <= 2 {
        return;
    }
    let width = UnicodeWidthStr::width(&editor.note[..editor.cursor]);
    let max = usize::from(area.width.saturating_sub(3));
    let x = area.x + 1 + u16::try_from(width.min(max)).unwrap_or(0);
    frame.set_cursor_position((x, area.y + 1));
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
            return Err(error).context("failed to initialize popup terminal");
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

    use crate::model::SCHEMA_VERSION;

    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press)
    }

    #[test]
    fn capture_keys_choose_the_expected_completion() {
        let mut editor = EditorState::default();

        assert_eq!(
            editor.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Completion::Insert)
        );
        assert_eq!(
            editor.handle_key(key(KeyCode::Enter, KeyModifiers::ALT)),
            Some(Completion::Collect)
        );
        assert_eq!(
            editor.handle_key(key(KeyCode::Char('p'), KeyModifiers::CONTROL)),
            Some(Completion::PasteAll)
        );
        assert_eq!(
            editor.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Completion::Cancel)
        );
    }

    #[test]
    fn editing_is_unicode_safe_and_paste_stays_on_one_line() {
        let mut editor = EditorState::default();
        editor.handle_key(key(KeyCode::Char('é'), KeyModifiers::NONE));
        editor.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
        editor.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
        editor.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
        editor.handle_paste("one\ntwo");

        assert_eq!(editor.note(), "one twox");
    }

    #[test]
    fn compact_render_contains_preview_count_and_help() {
        let draft = Draft {
            schema_version: SCHEMA_VERSION,
            id: "a".repeat(32),
            source_text: "one\ntwo\nthree\nfour\nfive".into(),
            pane_id: "w1:p1".into(),
            scope: "b".repeat(64),
            created_at_ms: 0,
        };
        let backend = TestBackend::new(64, 14);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &draft, 2, &EditorState::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let content = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(content.contains("Copied text · 2 collected"));
        assert!(content.contains("> ... preview truncated"));
        assert!(content.contains("option+enter collect"));
        assert!(content.contains("ctrl+p paste all"));
    }
}
