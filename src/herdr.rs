use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::PopupSize;
use crate::model::PaneSnapshot;
use crate::snapshot::{assemble, rows, ScrollState, CAPTURE_ROWS};

pub const PLUGIN_ID: &str = "shadowfax.comments";
const MAX_API_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_API_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    UiBusy,
    PaneMissing,
    Command,
}

pub trait HerdrClient {
    fn capture_snapshot(&self, pane_id: &str) -> Result<PaneSnapshot>;
    fn open_annotation(&self, run_id: &str, popup: &PopupSize) -> Result<()>;
    fn open_inline_annotation(&self, run_id: &str, popup: &PopupSize) -> Result<()>;
    fn open_review(&self, review_id: &str, popup: &PopupSize) -> Result<()>;
    fn send_input(&self, pane_id: &str, text: &str) -> Result<()>;
    fn notify(&self, title: &str, body: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct CliHerdr {
    bin: PathBuf,
    socket_path: PathBuf,
}

impl CliHerdr {
    pub fn new(bin: impl Into<PathBuf>, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            bin: resolve_herdr_bin(bin.into()),
            socket_path: socket_path.into(),
        }
    }

    fn run(&self, args: &[String], operation: &str) -> Result<Output> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .with_context(|| format!("failed to start Herdr while trying to {operation}"))?;
        if output.status.success() {
            return Ok(output);
        }
        fail(operation, &output)
    }

    fn read_pane(
        &self,
        pane_id: &str,
        source: &str,
        format: &str,
        lines: Option<u32>,
    ) -> Result<String> {
        let output = self.run(
            &pane_read_args(pane_id, source, format, lines),
            "capture pane text",
        )?;
        String::from_utf8(output.stdout).context("Herdr returned non-UTF-8 pane text")
    }

    fn scroll_state(&self, pane_id: &str) -> Result<Option<ScrollState>> {
        let output = self.run(&pane_get_args(pane_id), "inspect pane scroll state")?;
        let response: PaneEnvelope =
            serde_json::from_slice(&output.stdout).context("invalid Herdr pane response")?;
        response
            .result
            .pane
            .scroll
            .map(RawScroll::try_into)
            .transpose()
    }

    pub fn client_width(&self, pane_id: &str) -> Result<u16> {
        let output = self.run(&pane_layout_args(pane_id), "measure the Herdr client")?;
        let response: LayoutEnvelope =
            serde_json::from_slice(&output.stdout).context("invalid Herdr layout response")?;
        Ok(response
            .result
            .layout
            .area
            .x
            .saturating_add(response.result.layout.area.width))
    }
}

fn resolve_herdr_bin(bin: PathBuf) -> PathBuf {
    if bin.is_file() {
        return bin;
    }
    bin.parent()
        .map(|parent| parent.join("herdr"))
        .filter(|candidate| candidate.is_file())
        .unwrap_or(bin)
}

impl HerdrClient for CliHerdr {
    fn capture_snapshot(&self, pane_id: &str) -> Result<PaneSnapshot> {
        let scroll = self.scroll_state(pane_id)?;
        let recent_text = self.read_pane(pane_id, "recent", "text", Some(CAPTURE_ROWS))?;
        let recent_ansi = self.read_pane(pane_id, "recent", "ansi", Some(CAPTURE_ROWS))?;
        let visible_text = self.read_pane(pane_id, "visible", "text", None)?;
        let visible_ansi = self.read_pane(pane_id, "visible", "ansi", None)?;
        let scroll = scroll.unwrap_or_else(|| ScrollState {
            offset_from_bottom: 0,
            max_offset_from_bottom: 0,
            viewport_rows: rows(&visible_text).len(),
        });
        assemble(
            &recent_text,
            &recent_ansi,
            &visible_text,
            &visible_ansi,
            scroll,
        )
    }

    fn open_annotation(&self, run_id: &str, popup: &PopupSize) -> Result<()> {
        self.run(
            &annotation_popup_args(run_id, popup),
            "open the annotation popup",
        )?;
        Ok(())
    }

    fn open_inline_annotation(&self, run_id: &str, popup: &PopupSize) -> Result<()> {
        self.run(
            &inline_annotation_popup_args(run_id, popup),
            "open the inline comment popup",
        )?;
        Ok(())
    }

    fn open_review(&self, review_id: &str, popup: &PopupSize) -> Result<()> {
        self.run(
            &review_popup_args(review_id, popup),
            "open the review popup",
        )?;
        Ok(())
    }

    fn send_input(&self, pane_id: &str, text: &str) -> Result<()> {
        send_socket_request(&self.socket_path, pane_id, text)
    }

    fn notify(&self, title: &str, body: &str) -> Result<()> {
        self.run(&notification_args(title, body), "show a notification")?;
        Ok(())
    }
}

pub fn annotation_popup_args(run_id: &str, popup: &PopupSize) -> Vec<String> {
    annotation_popup_args_with_mode(run_id, popup, false)
}

pub fn inline_annotation_popup_args(run_id: &str, popup: &PopupSize) -> Vec<String> {
    annotation_popup_args_with_mode(run_id, popup, true)
}

fn annotation_popup_args_with_mode(run_id: &str, popup: &PopupSize, inline: bool) -> Vec<String> {
    let mut args = vec![
        "plugin".into(),
        "pane".into(),
        "open".into(),
        "--plugin".into(),
        PLUGIN_ID.into(),
        "--entrypoint".into(),
        "capture".into(),
        "--placement".into(),
        "popup".into(),
        "--width".into(),
        popup.width.clone(),
        "--height".into(),
        popup.height.clone(),
        "--env".into(),
        format!("HERDR_COMMENTS_RUN_ID={run_id}"),
    ];
    if inline {
        args.push("--env".into());
        args.push("HERDR_COMMENTS_INLINE=1".into());
    }
    args.push("--focus".into());
    args
}

pub fn review_popup_args(review_id: &str, popup: &PopupSize) -> Vec<String> {
    vec![
        "plugin".into(),
        "pane".into(),
        "open".into(),
        "--plugin".into(),
        PLUGIN_ID.into(),
        "--entrypoint".into(),
        "review".into(),
        "--placement".into(),
        "popup".into(),
        "--width".into(),
        popup.width.clone(),
        "--height".into(),
        popup.height.clone(),
        "--env".into(),
        format!("HERDR_COMMENTS_REVIEW_ID={review_id}"),
        "--focus".into(),
    ]
}

pub fn pane_read_args(
    pane_id: &str,
    source: &str,
    format: &str,
    lines: Option<u32>,
) -> Vec<String> {
    let mut args = vec![
        "pane".into(),
        "read".into(),
        pane_id.into(),
        "--source".into(),
        source.into(),
        "--format".into(),
        format.into(),
    ];
    if let Some(lines) = lines {
        args.push("--lines".into());
        args.push(lines.to_string());
    }
    args
}

pub fn pane_get_args(pane_id: &str) -> Vec<String> {
    vec!["pane".into(), "get".into(), pane_id.into()]
}

pub fn pane_layout_args(pane_id: &str) -> Vec<String> {
    vec![
        "pane".into(),
        "layout".into(),
        "--pane".into(),
        pane_id.into(),
    ]
}

pub fn pane_send_input_request(request_id: &str, pane_id: &str, text: &str) -> Result<String> {
    let request = serde_json::to_string(&PaneSendInputRequest {
        id: request_id,
        method: "pane.send_input",
        params: PaneSendInputParams {
            pane_id,
            text,
            keys: Vec::new(),
        },
    })?;
    if request.len().saturating_add(1) > MAX_API_REQUEST_BYTES {
        bail!("could not insert comments: the review exceeds Herdr's socket request limit");
    }
    Ok(request)
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

fn fail<T>(operation: &str, output: &Output) -> Result<T> {
    fail_with_kind(operation, classify_failure(&diagnostic(output)))
}

fn fail_with_kind<T>(operation: &str, kind: FailureKind) -> Result<T> {
    match kind {
        FailureKind::UiBusy => {
            bail!("could not {operation}: close the active Herdr popup or modal")
        }
        FailureKind::PaneMissing => {
            bail!("could not {operation}: the originating pane no longer exists")
        }
        FailureKind::Command => bail!("could not {operation}: Herdr rejected the request"),
    }
}

fn diagnostic(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn validate_pane_id(pane_id: &str) -> Result<()> {
    if pane_id.trim().is_empty() || pane_id.chars().any(char::is_control) {
        bail!("Herdr returned an invalid pane identifier");
    }
    Ok(())
}

fn send_socket_request(socket_path: &Path, pane_id: &str, text: &str) -> Result<()> {
    validate_pane_id(pane_id)?;
    let request_id = format!("herdr-comments-{}", Uuid::new_v4().simple());
    let request = pane_send_input_request(&request_id, pane_id, text)?;
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("failed to connect to Herdr at {}", socket_path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut response = String::new();
    BufReader::new(stream)
        .take((MAX_API_RESPONSE_BYTES + 1) as u64)
        .read_line(&mut response)?;
    if response.is_empty() {
        bail!("could not insert comments: Herdr closed the API socket without a response");
    }
    if response.len() > MAX_API_RESPONSE_BYTES || !response.ends_with('\n') {
        bail!("could not insert comments: Herdr returned an invalid framed response");
    }
    let response: ApiResponse =
        serde_json::from_str(&response).context("Herdr returned an invalid API response")?;
    if response.id != request_id {
        bail!("could not insert comments: Herdr returned a mismatched response");
    }
    if let Some(error) = response.error {
        return fail_api("insert comments", &error.code, &error.message);
    }
    if response.result.is_some_and(|result| result.kind == "ok") {
        return Ok(());
    }
    bail!("could not insert comments: Herdr returned an unexpected response")
}

fn fail_api<T>(operation: &str, code: &str, message: &str) -> Result<T> {
    fail_with_kind(operation, classify_failure(&format!("{code}\n{message}")))
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

#[derive(Deserialize)]
struct PaneEnvelope {
    result: PaneResult,
}

#[derive(Deserialize)]
struct PaneResult {
    pane: RawPane,
}

#[derive(Deserialize)]
struct RawPane {
    scroll: Option<RawScroll>,
}

#[derive(Deserialize)]
struct LayoutEnvelope {
    result: LayoutResult,
}

#[derive(Deserialize)]
struct LayoutResult {
    layout: ClientLayout,
}

#[derive(Deserialize)]
struct ClientLayout {
    area: LayoutArea,
}

#[derive(Deserialize)]
struct LayoutArea {
    x: u16,
    width: u16,
}

#[derive(Deserialize)]
struct RawScroll {
    offset_from_bottom: u64,
    max_offset_from_bottom: u64,
    viewport_rows: u64,
}

impl TryFrom<RawScroll> for ScrollState {
    type Error = anyhow::Error;

    fn try_from(value: RawScroll) -> Result<Self> {
        Ok(Self {
            offset_from_bottom: value.offset_from_bottom.try_into()?,
            max_offset_from_bottom: value.max_offset_from_bottom.try_into()?,
            viewport_rows: value.viewport_rows.try_into()?,
        })
    }
}

#[derive(Serialize)]
struct PaneSendInputRequest<'a> {
    id: &'a str,
    method: &'static str,
    params: PaneSendInputParams<'a>,
}

#[derive(Serialize)]
struct PaneSendInputParams<'a> {
    pane_id: &'a str,
    text: &'a str,
    keys: Vec<&'a str>,
}

#[derive(Deserialize)]
struct ApiResponse {
    id: String,
    result: Option<ApiResult>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiResult {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct ApiError {
    code: String,
    message: String,
}
