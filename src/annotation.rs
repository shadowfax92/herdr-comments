use anyhow::Result;

use crate::context::ActionContext;
use crate::herdr::{ClosePaneResult, HerdrClient};
use crate::model::AnnotationRun;
use crate::store::{scope_id, session_key, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationStart {
    Opened(Box<AnnotationRun>),
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewTarget {
    pub pane_id: String,
    pub scope: String,
    pub annotation_run_id: Option<String>,
    pub overlay_pane_id: Option<String>,
}

pub struct AnnotationService<'a, H> {
    store: &'a Store,
    herdr: &'a H,
}

impl<'a, H: HerdrClient> AnnotationService<'a, H> {
    pub fn new(store: &'a Store, herdr: &'a H) -> Self {
        Self { store, herdr }
    }

    pub fn start(&self, pane_id: &str, scope: &str, session_key: &str) -> Result<AnnotationStart> {
        self.store.cleanup_transients()?;
        if let Some(active) = self.store.active_annotation(session_key)? {
            let outcome = self.herdr.close_plugin_pane(&active.overlay_pane_id)?;
            self.store.delete_annotation(&active.run_id)?;
            if outcome == ClosePaneResult::Closed {
                return Ok(AnnotationStart::Closed);
            }
        }

        let snapshot = self.herdr.capture_snapshot(pane_id)?;
        let run = self
            .store
            .create_annotation(pane_id, scope, session_key, snapshot)?;
        let overlay_pane_id = match self.herdr.open_annotation(&run.id) {
            Ok(pane_id) => pane_id,
            Err(error) => {
                let _ = self.store.delete_annotation(&run.id);
                return Err(error);
            }
        };
        match self.store.attach_annotation(&run.id, &overlay_pane_id) {
            Ok(run) => Ok(AnnotationStart::Opened(Box::new(run))),
            Err(error) => {
                let _ = self.herdr.close_plugin_pane(&overlay_pane_id);
                let _ = self.store.delete_annotation(&run.id);
                Err(error)
            }
        }
    }
}

pub fn review_target(store: &Store, context: &ActionContext) -> Result<ReviewTarget> {
    let session_key = session_key(&context.session_identity);
    if let Some(run) = store.annotation_for_overlay(&session_key, &context.pane_id)? {
        return Ok(ReviewTarget {
            pane_id: run.pane_id,
            scope: run.scope,
            annotation_run_id: Some(run.id),
            overlay_pane_id: run.overlay_pane_id,
        });
    }
    Ok(ReviewTarget {
        pane_id: context.pane_id.clone(),
        scope: scope_id(&context.session_identity, &context.pane_id),
        annotation_run_id: None,
        overlay_pane_id: None,
    })
}
