use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PluginContext {
    focused_pane_id: Option<String>,
    focused_pane_cwd: Option<String>,
    selected_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionContext {
    pub pane_id: String,
    pub pane_cwd: Option<PathBuf>,
    pub selected_text: Option<String>,
    pub state_dir: PathBuf,
    pub herdr_bin: PathBuf,
    pub session_identity: String,
}

impl ActionContext {
    pub fn from_env() -> Result<Self> {
        Self::from_values(
            &std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
                .context("HERDR_PLUGIN_CONTEXT_JSON is missing")?,
            &std::env::var("HERDR_PLUGIN_STATE_DIR")
                .context("HERDR_PLUGIN_STATE_DIR is missing")?,
            &std::env::var("HERDR_BIN_PATH").context("HERDR_BIN_PATH is missing")?,
            &std::env::var("HERDR_SOCKET_PATH").context("HERDR_SOCKET_PATH is missing")?,
        )
    }

    pub fn from_values(
        raw_context: &str,
        state_dir: &str,
        herdr_bin: &str,
        session_identity: &str,
    ) -> Result<Self> {
        let parsed: PluginContext =
            serde_json::from_str(raw_context).context("invalid HERDR_PLUGIN_CONTEXT_JSON")?;
        let pane_id = required(parsed.focused_pane_id.as_deref(), "focused pane")?;
        let state_dir = required(Some(state_dir), "plugin state directory")?;
        let herdr_bin = required(Some(herdr_bin), "Herdr binary")?;
        let session_identity = required(Some(session_identity), "Herdr session identity")?;

        Ok(Self {
            pane_id,
            pane_cwd: parsed
                .focused_pane_cwd
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from),
            selected_text: parsed.selected_text,
            state_dir: PathBuf::from(state_dir),
            herdr_bin: PathBuf::from(herdr_bin),
            session_identity,
        })
    }
}

fn required(value: Option<&str>, label: &str) -> Result<String> {
    let value = value.unwrap_or_default().trim();
    if value.is_empty() {
        bail!("Herdr did not provide a {label}");
    }
    Ok(value.to_owned())
}
