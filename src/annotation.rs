use anyhow::Result;

use crate::config::PopupSize;
use crate::context::ActionContext;
use crate::format::normalize_source;
use crate::herdr::HerdrClient;
use crate::model::{AnnotationRun, PaneSnapshot};
use crate::store::{scope_id, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewTarget {
    pub pane_id: String,
    pub scope: String,
}

pub struct AnnotationService<'a, H> {
    store: &'a Store,
    herdr: &'a H,
}

impl<'a, H: HerdrClient> AnnotationService<'a, H> {
    pub fn new(store: &'a Store, herdr: &'a H) -> Self {
        Self { store, herdr }
    }

    pub fn start(&self, pane_id: &str, scope: &str, popup: &PopupSize) -> Result<AnnotationRun> {
        self.store.cleanup_transients()?;
        let snapshot = self.herdr.capture_snapshot(pane_id)?;
        let run = self.store.create_annotation(pane_id, scope, snapshot)?;
        if let Err(error) = self.herdr.open_annotation(&run.id, popup) {
            let _ = self.store.delete_annotation(&run.id);
            return Err(error);
        }
        Ok(run)
    }

    pub fn start_inline(
        &self,
        pane_id: &str,
        scope: &str,
        selected_text: &str,
        popup: &PopupSize,
    ) -> Result<AnnotationRun> {
        self.store.cleanup_transients()?;
        let source = normalize_source(selected_text)?;
        let snapshot = PaneSnapshot {
            viewport_rows: source.split('\n').count(),
            ansi: source.clone(),
            text: source,
            initial_top: 0,
            history_limited: false,
        };
        let run = self.store.create_annotation(pane_id, scope, snapshot)?;
        if let Err(error) = self.herdr.open_inline_annotation(&run.id, popup) {
            let _ = self.store.delete_annotation(&run.id);
            return Err(error);
        }
        Ok(run)
    }
}

pub fn review_target(context: &ActionContext) -> ReviewTarget {
    ReviewTarget {
        pane_id: context.pane_id.clone(),
        scope: scope_id(&context.session_identity, &context.pane_id),
    }
}
