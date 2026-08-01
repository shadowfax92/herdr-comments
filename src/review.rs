use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::annotation::ReviewTarget;
use crate::format::{format_collection, format_comment, validate_review};
use crate::herdr::HerdrClient;
use crate::model::ReviewSession;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewStart {
    Empty,
    Opened(ReviewSession),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewResult {
    Inserted { count: usize },
    Cancelled,
}

pub struct ReviewService<'a, H> {
    store: &'a Store,
    herdr: &'a H,
}

impl<'a, H: HerdrClient> ReviewService<'a, H> {
    pub fn new(store: &'a Store, herdr: &'a H) -> Self {
        Self { store, herdr }
    }

    pub fn start(&self, target: &ReviewTarget) -> Result<ReviewStart> {
        self.store.cleanup_transients()?;
        let comments = self.store.list_comments(&target.scope)?;
        if comments.is_empty() {
            let _ = self
                .herdr
                .notify("Herdr Comments", "No comments are collected for this pane.");
            return Ok(ReviewStart::Empty);
        }

        let formatted = comments
            .iter()
            .map(|comment| format_comment(&comment.source_text, &comment.note))
            .collect::<Result<Vec<_>>>()?;
        let comment_ids = comments.iter().map(|comment| comment.id.clone()).collect();
        let review = self.store.create_review(
            &target.pane_id,
            &target.scope,
            comment_ids,
            &format_collection(&formatted),
            target.annotation_run_id.clone(),
            target.overlay_pane_id.clone(),
        )?;
        if let Err(error) = self.herdr.open_review(&review.id) {
            let _ = self.store.delete_review(&review.id);
            return Err(error);
        }
        Ok(ReviewStart::Opened(review))
    }

    pub fn complete(&self, review_id: &str) -> Result<ReviewResult> {
        let review = self.store.load_review(review_id)?;
        if !self.store.review_is_confirmed(review_id)? {
            self.store.delete_review(review_id)?;
            return Ok(ReviewResult::Cancelled);
        }

        let markdown = self.store.read_review_markdown(review_id)?;
        validate_review(&markdown)?;
        if markdown.trim().is_empty() {
            self.store.delete_review(review_id)?;
            let _ = self.herdr.notify(
                "Herdr Comments",
                "The review was blank, so no comments were inserted.",
            );
            return Ok(ReviewResult::Cancelled);
        }

        self.herdr.send_input(&review.pane_id, &markdown)?;
        self.store
            .delete_comments(&review.scope, &review.comment_ids)
            .context("comments were inserted, but cleanup failed; do not retry this review")?;
        self.store
            .delete_review(review_id)
            .context("comments were inserted, but cleanup failed; do not retry this review")?;
        if let Some(overlay_pane_id) = review.overlay_pane_id.as_deref() {
            self.herdr
                .close_plugin_pane(overlay_pane_id)
                .context("comments were inserted, but the annotation view could not close")?;
        }
        if let Some(run_id) = review.annotation_run_id.as_deref() {
            self.store
                .delete_annotation(run_id)
                .context("comments were inserted, but annotation cleanup failed")?;
        }
        let _ = self.herdr.notify(
            "Herdr Comments",
            &format!(
                "Inserted {} collected comment(s).",
                review.comment_ids.len()
            ),
        );
        Ok(ReviewResult::Inserted {
            count: review.comment_ids.len(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct NvimEditor {
    binary: OsString,
    executable: PathBuf,
    script: PathBuf,
}

impl NvimEditor {
    pub fn new(
        binary: impl Into<OsString>,
        executable: impl Into<PathBuf>,
        script: impl Into<PathBuf>,
    ) -> Self {
        Self {
            binary: binary.into(),
            executable: executable.into(),
            script: script.into(),
        }
    }

    pub fn run(&self, review_id: &str, markdown_path: &Path) -> Result<()> {
        if !self.script.is_file() {
            bail!(
                "Neovim review script is missing at {}",
                self.script.display()
            );
        }
        let status = Command::new(&self.binary)
            .args(Self::arguments(markdown_path))
            .env("HERDR_COMMENTS_REVIEW_ID", review_id)
            .env("HERDR_COMMENTS_BIN", &self.executable)
            .env("HERDR_COMMENTS_REVIEW_LUA", &self.script)
            .status()
            .context("failed to start Neovim for comment review")?;
        if !status.success() {
            bail!("Neovim exited before the comment review completed");
        }
        Ok(())
    }

    fn arguments(markdown_path: &Path) -> Vec<OsString> {
        vec![
            OsStr::new("-c").to_owned(),
            OsStr::new("lua dofile(vim.env.HERDR_COMMENTS_REVIEW_LUA)").to_owned(),
            markdown_path.as_os_str().to_owned(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use super::NvimEditor;

    #[test]
    fn neovim_receives_a_script_and_private_file_instead_of_comment_text() {
        assert_eq!(
            NvimEditor::arguments(Path::new("/private/reviews/abc.md")),
            vec![
                OsString::from("-c"),
                OsString::from("lua dofile(vim.env.HERDR_COMMENTS_REVIEW_LUA)"),
                OsString::from("/private/reviews/abc.md"),
            ]
        );
    }
}
