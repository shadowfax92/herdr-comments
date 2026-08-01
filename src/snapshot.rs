use anyhow::{bail, Result};

use crate::format::{normalize_snapshot, MAX_SNAPSHOT_BYTES};
use crate::model::PaneSnapshot;

pub const CAPTURE_ROWS: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollState {
    pub offset_from_bottom: usize,
    pub max_offset_from_bottom: usize,
    pub viewport_rows: usize,
}

pub fn assemble(
    recent_text: &str,
    recent_ansi: &str,
    visible_text: &str,
    visible_ansi: &str,
    scroll: ScrollState,
) -> Result<PaneSnapshot> {
    let recent_is_empty = recent_text.trim().is_empty();
    let recent_text = if recent_is_empty {
        normalize_snapshot(visible_text)?
    } else {
        normalize_snapshot(recent_text)?
    };
    let recent_ansi = if recent_is_empty {
        visible_ansi
    } else {
        recent_ansi
    };
    let viewport_rows = scroll.viewport_rows.max(1);
    let recent_rows = row_count(&recent_text);
    let current_view_is_available = scroll.offset_from_bottom == 0
        || recent_rows >= viewport_rows.saturating_add(scroll.offset_from_bottom);

    let (text, ansi, initial_top) = if current_view_is_available {
        let initial_top = recent_rows
            .saturating_sub(viewport_rows)
            .saturating_sub(scroll.offset_from_bottom);
        (recent_text, normalize_ansi(recent_ansi)?, initial_top)
    } else {
        (
            normalize_snapshot(visible_text)?,
            normalize_ansi(visible_ansi)?,
            0,
        )
    };
    let ansi = if row_count(&ansi) == row_count(&text) {
        ansi
    } else {
        text.clone()
    };
    let history_rows = scroll.max_offset_from_bottom.saturating_add(viewport_rows);

    Ok(PaneSnapshot {
        text,
        ansi,
        initial_top,
        viewport_rows,
        history_limited: !current_view_is_available
            || (scroll.max_offset_from_bottom > 0 && history_rows > recent_rows),
    })
}

pub fn rows(text: &str) -> Vec<String> {
    let mut rows = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
    if text.ends_with('\n') && rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

fn row_count(text: &str) -> usize {
    rows(text).len()
}

fn normalize_ansi(text: &str) -> Result<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.len() > MAX_SNAPSHOT_BYTES.saturating_mul(4) {
        bail!("the styled pane snapshot is too large");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_history_opens_at_the_source_viewport() {
        let snapshot = assemble(
            "one\ntwo\nthree\nfour\nfive",
            "one\ntwo\nthree\nfour\nfive",
            "two\nthree",
            "two\nthree",
            ScrollState {
                offset_from_bottom: 2,
                max_offset_from_bottom: 3,
                viewport_rows: 2,
            },
        )
        .unwrap();

        assert_eq!(snapshot.initial_top, 1);
        assert!(!snapshot.history_limited);
        assert_eq!(rows(&snapshot.text)[snapshot.initial_top], "two");
    }

    #[test]
    fn visible_view_is_kept_when_recent_history_cannot_reach_it() {
        let snapshot = assemble(
            "new one\nnew two",
            "new one\nnew two",
            "old one\nold two",
            "\u{1b}[31mold one\u{1b}[0m\nold two",
            ScrollState {
                offset_from_bottom: 50,
                max_offset_from_bottom: 80,
                viewport_rows: 2,
            },
        )
        .unwrap();

        assert_eq!(snapshot.text, "old one\nold two");
        assert_eq!(snapshot.initial_top, 0);
        assert!(snapshot.history_limited);
    }

    #[test]
    fn visible_content_is_used_when_recent_output_is_empty() {
        let snapshot = assemble(
            "",
            "",
            "current screen",
            "\u{1b}[32mcurrent screen\u{1b}[0m",
            ScrollState {
                offset_from_bottom: 0,
                max_offset_from_bottom: 0,
                viewport_rows: 1,
            },
        )
        .unwrap();

        assert_eq!(snapshot.text, "current screen");
        assert!(snapshot.ansi.contains("\u{1b}[32m"));
    }

    #[test]
    fn recent_history_remains_available_when_the_current_screen_is_blank() {
        let snapshot = assemble(
            "earlier output",
            "\u{1b}[32mearlier output\u{1b}[0m",
            "",
            "",
            ScrollState {
                offset_from_bottom: 0,
                max_offset_from_bottom: 0,
                viewport_rows: 10,
            },
        )
        .unwrap();

        assert_eq!(snapshot.text, "earlier output");
        assert!(!snapshot.history_limited);
    }

    #[test]
    fn a_partially_filled_unscrolled_screen_is_not_history_limited() {
        let snapshot = assemble(
            "prompt\noutput\nprompt",
            "prompt\noutput\nprompt",
            "prompt\noutput\nprompt",
            "prompt\noutput\nprompt",
            ScrollState {
                offset_from_bottom: 0,
                max_offset_from_bottom: 0,
                viewport_rows: 70,
            },
        )
        .unwrap();

        assert_eq!(snapshot.initial_top, 0);
        assert!(!snapshot.history_limited);
    }
}
