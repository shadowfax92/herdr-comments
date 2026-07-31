pub mod capture;
pub mod clipboard;
pub mod context;
pub mod format;
pub mod herdr;
pub mod model;
pub mod store;
pub mod tui;

use anyhow::{Context, Result};

use capture::CaptureService;
use context::ActionContext;
use herdr::CliHerdr;
use store::Store;

pub fn capture_action() -> Result<()> {
    let context = ActionContext::from_env()?;
    let store = Store::open(&context.state_dir)?;
    let herdr = CliHerdr::new(&context.herdr_bin);
    let source_text = clipboard::read_macos()?;
    let scope = store::scope_id(&context.session_identity, &context.pane_id);
    CaptureService::new(&store, &herdr)
        .start(&source_text, &context.pane_id, &scope)
        .map(|_| ())
}

pub fn capture_popup() -> Result<()> {
    let draft_id =
        std::env::var("HERDR_COMMENTS_DRAFT_ID").context("HERDR_COMMENTS_DRAFT_ID is missing")?;
    let state_dir =
        std::env::var("HERDR_PLUGIN_STATE_DIR").context("HERDR_PLUGIN_STATE_DIR is missing")?;
    let herdr_bin = std::env::var("HERDR_BIN_PATH").context("HERDR_BIN_PATH is missing")?;
    let store = Store::open(state_dir)?;
    let herdr = CliHerdr::new(herdr_bin);
    tui::run_capture(&store, &herdr, &draft_id)
}
