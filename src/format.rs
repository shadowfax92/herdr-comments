use anyhow::{bail, Result};

pub const MAX_SOURCE_BYTES: usize = 200 * 1024;
pub const MAX_REVIEW_BYTES: usize = 1024 * 1024;

pub fn normalize_source(text: &str) -> Result<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.trim().is_empty() {
        bail!("the copied text is empty");
    }
    if normalized.len() > MAX_SOURCE_BYTES {
        bail!(
            "the copied text is {} bytes; select {} bytes or fewer",
            normalized.len(),
            MAX_SOURCE_BYTES
        );
    }
    ensure_safe_text(&normalized, true)?;
    Ok(normalized)
}

pub fn normalize_note(note: &str) -> Result<String> {
    let note = note.trim();
    if note.is_empty() {
        bail!("write a comment before continuing");
    }
    if note.contains(['\n', '\r']) {
        bail!("comments in the capture popup must fit on one line");
    }
    ensure_safe_text(note, false)?;
    Ok(note.to_owned())
}

pub fn validate_review(text: &str) -> Result<()> {
    if text.len() > MAX_REVIEW_BYTES {
        bail!("the reviewed comments exceed {MAX_REVIEW_BYTES} bytes");
    }
    ensure_safe_text(text, true)
}

pub fn format_comment(source_text: &str, note: &str) -> Result<String> {
    let source_text = normalize_source(source_text)?;
    let note = normalize_note(note)?;
    let quote = source_text
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
    Ok(format!("{quote}\n\n{note}\n"))
}

pub fn format_collection(comments: &[String]) -> String {
    comments
        .iter()
        .map(|comment| comment.trim_end_matches('\n'))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

pub fn preview_lines(text: &str, max_lines: usize, max_characters: usize) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let limited = normalized.chars().take(max_characters).collect::<String>();
    let all_lines = limited.split('\n').collect::<Vec<_>>();
    let mut output = all_lines
        .iter()
        .take(max_lines)
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>();
    if normalized.chars().count() > limited.chars().count() || all_lines.len() > max_lines {
        output.push("> ... preview truncated".into());
    }
    output
}

fn ensure_safe_text(text: &str, allow_layout: bool) -> Result<()> {
    if text
        .chars()
        .any(|ch| ch.is_control() && !(allow_layout && matches!(ch, '\n' | '\t')))
    {
        bail!("text contains unsupported control characters");
    }
    Ok(())
}
