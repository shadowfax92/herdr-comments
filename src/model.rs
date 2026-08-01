use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub text: String,
    pub ansi: String,
    pub initial_top: usize,
    pub viewport_rows: usize,
    pub history_limited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationRun {
    pub schema_version: u32,
    pub id: String,
    pub pane_id: String,
    pub scope: String,
    pub session_key: String,
    pub snapshot: PaneSnapshot,
    #[serde(default)]
    pub overlay_pane_id: Option<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveAnnotation {
    pub schema_version: u32,
    pub run_id: String,
    pub overlay_pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub schema_version: u32,
    pub id: String,
    pub source_text: String,
    pub note: String,
    pub created_at_ns: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSession {
    pub schema_version: u32,
    pub id: String,
    pub pane_id: String,
    pub scope: String,
    pub comment_ids: Vec<String>,
    #[serde(default)]
    pub annotation_run_id: Option<String>,
    #[serde(default)]
    pub overlay_pane_id: Option<String>,
    pub created_at_ms: u64,
}
