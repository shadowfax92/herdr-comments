use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{bail, Context, Result};

pub const PLUGIN_ID: &str = "shadowfax.comments";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    UiBusy,
    PaneMissing,
    Command,
}

pub trait HerdrClient {
    fn open_capture(&self, draft_id: &str) -> Result<()>;
    fn open_review(&self, review_id: &str) -> Result<()>;
    fn paste_text(&self, pane_id: &str, text: &str) -> Result<()>;
    fn notify(&self, title: &str, body: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct CliHerdr {
    bin: PathBuf,
}

impl CliHerdr {
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into() }
    }

    fn run(&self, args: &[String], operation: &str) -> Result<Output> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .with_context(|| format!("failed to start Herdr while trying to {operation}"))?;
        if output.status.success() {
            return Ok(output);
        }

        let diagnostic = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        match classify_failure(&diagnostic) {
            FailureKind::UiBusy => {
                bail!("could not {operation}: close the active Herdr popup or modal")
            }
            FailureKind::PaneMissing => {
                bail!("could not {operation}: the originating pane no longer exists")
            }
            FailureKind::Command => bail!("could not {operation}: Herdr rejected the request"),
        }
    }
}

impl HerdrClient for CliHerdr {
    fn open_capture(&self, draft_id: &str) -> Result<()> {
        self.run(&capture_popup_args(draft_id), "open the comment popup")?;
        Ok(())
    }

    fn open_review(&self, review_id: &str) -> Result<()> {
        self.run(&review_popup_args(review_id), "open the review popup")?;
        Ok(())
    }

    fn paste_text(&self, pane_id: &str, text: &str) -> Result<()> {
        self.run(&paste_args(pane_id, text), "insert comments")?;
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) -> Result<()> {
        self.run(&notification_args(title, body), "show a notification")?;
        Ok(())
    }
}

pub fn capture_popup_args(draft_id: &str) -> Vec<String> {
    popup_args("capture", "48%", "30%", "HERDR_COMMENTS_DRAFT_ID", draft_id)
}

pub fn review_popup_args(review_id: &str) -> Vec<String> {
    popup_args(
        "review",
        "90%",
        "85%",
        "HERDR_COMMENTS_REVIEW_ID",
        review_id,
    )
}

fn popup_args(entrypoint: &str, width: &str, height: &str, key: &str, id: &str) -> Vec<String> {
    vec![
        "plugin".into(),
        "pane".into(),
        "open".into(),
        "--plugin".into(),
        PLUGIN_ID.into(),
        "--entrypoint".into(),
        entrypoint.into(),
        "--placement".into(),
        "popup".into(),
        "--width".into(),
        width.into(),
        "--height".into(),
        height.into(),
        "--env".into(),
        format!("{key}={id}"),
        "--focus".into(),
    ]
}

pub fn paste_args(pane_id: &str, text: &str) -> Vec<String> {
    vec!["pane".into(), "paste".into(), pane_id.into(), text.into()]
}

fn notification_args(title: &str, body: &str) -> Vec<String> {
    vec![
        "notification".into(),
        "show".into(),
        title.into(),
        "--body".into(),
        body.into(),
        "--sound".into(),
        "none".into(),
    ]
}

pub fn classify_failure(output: &str) -> FailureKind {
    let output = output.to_ascii_lowercase();
    if output.contains("ui_busy") {
        FailureKind::UiBusy
    } else if output.contains("pane_not_found")
        || output.contains("pane not found")
        || output.contains("no such pane")
    {
        FailureKind::PaneMissing
    } else {
        FailureKind::Command
    }
}

pub fn plugin_root() -> Result<PathBuf> {
    std::env::var_os("HERDR_PLUGIN_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .filter(|path| path.is_dir())
        .context("Herdr plugin root is unavailable")
}

pub fn review_script(root: &Path) -> PathBuf {
    root.join("nvim/review.lua")
}
