pub mod annotation;
pub mod config;
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
use config::{LoadedConfig, CAPTURE_POPUP, REVIEW_POPUP};
use context::ActionContext;
use herdr::CliHerdr;
use review::{NvimEditor, ReviewService};
use store::Store;

pub fn capture_action() -> Result<()> {
    let context = ActionContext::from_env()?;
    let store = Store::open(&context.state_dir)?;
    let herdr = CliHerdr::new(&context.herdr_bin, &context.session_identity);
    let scope = store::scope_id(&context.session_identity, &context.pane_id);
    let config = LoadedConfig::load()?;
    let popup = config.popup(CAPTURE_POPUP, herdr.client_width(&context.pane_id).ok())?;
    let service = AnnotationService::new(&store, &herdr);
    if config.inline_comments() {
        let selected_text = context
            .selected_text
            .as_deref()
            .context("inline comments require a selection in Herdr Copy mode")?;
        service
            .start_inline(&context.pane_id, &scope, selected_text, &popup)
            .map(|_| ())
    } else {
        service.start(&context.pane_id, &scope, &popup).map(|_| ())
    }
}

pub fn capture_popup() -> Result<()> {
    let run_id =
        std::env::var("HERDR_COMMENTS_RUN_ID").context("HERDR_COMMENTS_RUN_ID is missing")?;
    let state_dir =
        std::env::var("HERDR_PLUGIN_STATE_DIR").context("HERDR_PLUGIN_STATE_DIR is missing")?;
    let store = Store::open(state_dir)?;
    if std::env::var("HERDR_COMMENTS_INLINE").is_ok_and(|value| value == "1") {
        tui::run_inline_annotation(&store, &run_id)
    } else {
        tui::run_annotation(&store, &run_id)
    }
}

pub fn review_action() -> Result<()> {
    let context = ActionContext::from_env()?;
    let store = Store::open(&context.state_dir)?;
    let herdr = CliHerdr::new(&context.herdr_bin, &context.session_identity);
    let target = review_target(&context);
    let popup =
        LoadedConfig::load()?.popup(REVIEW_POPUP, herdr.client_width(&context.pane_id).ok())?;
    ReviewService::new(&store, &herdr)
        .start(&target, &popup)
        .map(|_| ())
}

pub fn paste_action() -> Result<()> {
    let context = ActionContext::from_env()?;
    let store = Store::open(&context.state_dir)?;
    let herdr = CliHerdr::new(&context.herdr_bin, &context.session_identity);
    let target = review_target(&context);
    ReviewService::new(&store, &herdr)
        .paste_ready(&target)
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
        .finish(&review_id)
        .map(|_| ())
}

pub fn confirm_review(review_id: &str) -> Result<()> {
    let state_dir =
        std::env::var("HERDR_PLUGIN_STATE_DIR").context("HERDR_PLUGIN_STATE_DIR is missing")?;
    Store::open(state_dir)?.confirm_review(review_id)
}
