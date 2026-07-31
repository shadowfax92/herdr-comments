use anyhow::Result;

use crate::format::{format_collection, format_comment};
use crate::herdr::HerdrClient;
use crate::model::Draft;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Insert,
    Collect,
    PasteAll,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionResult {
    Inserted { count: usize },
    Collected { count: usize },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureStarted {
    pub draft: Draft,
    pub collected_count: usize,
}

pub struct CaptureService<'a, H> {
    store: &'a Store,
    herdr: &'a H,
}

impl<'a, H: HerdrClient> CaptureService<'a, H> {
    pub fn new(store: &'a Store, herdr: &'a H) -> Self {
        Self { store, herdr }
    }

    pub fn start(&self, source_text: &str, pane_id: &str, scope: &str) -> Result<CaptureStarted> {
        self.store.cleanup_transients()?;
        let collected_count = self.store.list_comments(scope)?.len();
        let draft = self.store.create_draft(source_text, pane_id, scope)?;
        self.herdr.open_capture(&draft.id)?;
        Ok(CaptureStarted {
            draft,
            collected_count,
        })
    }

    pub fn complete(
        &self,
        draft_id: &str,
        note: &str,
        completion: Completion,
    ) -> Result<CompletionResult> {
        let draft = self.store.load_draft(draft_id)?;
        match completion {
            Completion::Cancel => {
                self.store.delete_draft(&draft.id)?;
                Ok(CompletionResult::Cancelled)
            }
            Completion::Insert => {
                let markdown = format_comment(&draft.source_text, note)?;
                self.herdr.paste_text(&draft.pane_id, &markdown)?;
                self.store.delete_draft(&draft.id)?;
                let _ = self.herdr.notify(
                    "Comment inserted",
                    "Inserted without submitting the pane input.",
                );
                Ok(CompletionResult::Inserted { count: 1 })
            }
            Completion::Collect => {
                self.store
                    .add_comment(&draft.scope, &draft.source_text, note)?;
                self.store.delete_draft(&draft.id)?;
                let count = self.store.list_comments(&draft.scope)?.len();
                let _ = self.herdr.notify(
                    "Comment collected",
                    &format!("{count} comment{} collected for this pane.", plural(count)),
                );
                Ok(CompletionResult::Collected { count })
            }
            Completion::PasteAll => {
                let comments = self.store.list_comments(&draft.scope)?;
                let mut rendered = comments
                    .iter()
                    .map(|comment| format_comment(&comment.source_text, &comment.note))
                    .collect::<Result<Vec<_>>>()?;
                rendered.push(format_comment(&draft.source_text, note)?);
                let markdown = format_collection(&rendered);
                self.herdr.paste_text(&draft.pane_id, &markdown)?;
                let ids = comments
                    .iter()
                    .map(|comment| comment.id.clone())
                    .collect::<Vec<_>>();
                self.store.delete_comments(&draft.scope, &ids)?;
                self.store.delete_draft(&draft.id)?;
                let count = rendered.len();
                let _ = self.herdr.notify(
                    "Comments inserted",
                    &format!(
                        "Inserted {count} comment{} without submitting.",
                        plural(count)
                    ),
                );
                Ok(CompletionResult::Inserted { count })
            }
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
