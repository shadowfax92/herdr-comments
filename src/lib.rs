pub mod annotation;
pub mod context;
pub mod format;
pub mod herdr;
pub mod model;
pub mod review;
pub mod snapshot;
pub mod store;
pub mod tui;

use anyhow::{Context, Result};

use annotation::{review_target, AnnotationService};
use context::ActionContext;
use herdr::CliHerdr;
use review::{NvimEditor, ReviewService};
use store::Store;

pub fn capture_action() -> Result<()> {
    let context = ActionContext::from_env()?;
    let store = Store::open(&context.state_dir)?;
    let herdr = CliHerdr::new(&context.herdr_bin, &context.session_identity);
    let scope = store::scope_id(&context.session_identity, &context.pane_id);
    let session_key = store::session_key(&context.session_identity);
    AnnotationService::new(&store, &herdr)
        .start(&context.pane_id, &scope, &session_key)
        .map(|_| ())
}

pub fn capture_popup() -> Result<()> {
    let run_id =
        std::env::var("HERDR_COMMENTS_RUN_ID").context("HERDR_COMMENTS_RUN_ID is missing")?;
    let state_dir =
        std::env::var("HERDR_PLUGIN_STATE_DIR").context("HERDR_PLUGIN_STATE_DIR is missing")?;
    let herdr_bin = std::env::var("HERDR_BIN_PATH").context("HERDR_BIN_PATH is missing")?;
    let socket_path = std::env::var("HERDR_SOCKET_PATH").context("HERDR_SOCKET_PATH is missing")?;
    let store = Store::open(state_dir)?;
    let herdr = CliHerdr::new(herdr_bin, socket_path);
    tui::run_annotation(&store, &herdr, &run_id)
}

pub fn review_action() -> Result<()> {
    let context = ActionContext::from_env()?;
    let store = Store::open(&context.state_dir)?;
    let herdr = CliHerdr::new(&context.herdr_bin, &context.session_identity);
    let target = review_target(&store, &context)?;
    ReviewService::new(&store, &herdr)
        .start(&target)
        .map(|_| ())
}

pub fn review_popup() -> Result<()> {
    let review_id =
        std::env::var("HERDR_COMMENTS_REVIEW_ID").context("HERDR_COMMENTS_REVIEW_ID is missing")?;
    let state_dir =
        std::env::var("HERDR_PLUGIN_STATE_DIR").context("HERDR_PLUGIN_STATE_DIR is missing")?;
    let herdr_bin = std::env::var("HERDR_BIN_PATH").context("HERDR_BIN_PATH is missing")?;
    let socket_path = std::env::var("HERDR_SOCKET_PATH").context("HERDR_SOCKET_PATH is missing")?;
    let store = Store::open(state_dir)?;
    let herdr = CliHerdr::new(herdr_bin, socket_path);
    let root = herdr::plugin_root()?;
    let editor = NvimEditor::new(
        std::env::var_os("HERDR_COMMENTS_NVIM").unwrap_or_else(|| "nvim".into()),
        std::env::current_exe().context("herdr-comments executable path is unavailable")?,
        herdr::review_script(&root),
    );
    editor.run(&review_id, &store.review_markdown_path(&review_id)?)?;
    ReviewService::new(&store, &herdr)
        .complete(&review_id)
        .map(|_| ())
}

pub fn confirm_review(review_id: &str) -> Result<()> {
    let state_dir =
        std::env::var("HERDR_PLUGIN_STATE_DIR").context("HERDR_PLUGIN_STATE_DIR is missing")?;
    Store::open(state_dir)?.confirm_review(review_id)
}
