use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread;

use herdr_comments::config::PopupSize;
use herdr_comments::context::ActionContext;
use herdr_comments::herdr::{
    annotation_popup_args, classify_failure, pane_layout_args, pane_read_args,
    pane_send_input_request, review_popup_args, CliHerdr, FailureKind, HerdrClient,
};
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn action_context_reads_the_originating_pane_and_runtime_paths() {
    let context = ActionContext::from_values(
        r#"{"focused_pane_id":"w2:p3","focused_pane_cwd":"/tmp/project"}"#,
        "/tmp/comments-state",
        "/opt/bin/herdr",
        "/tmp/herdr.sock",
    )
    .unwrap();

    assert_eq!(context.pane_id, "w2:p3");
    assert_eq!(context.pane_cwd, Some(PathBuf::from("/tmp/project")));
    assert_eq!(context.state_dir, PathBuf::from("/tmp/comments-state"));
    assert_eq!(context.herdr_bin, PathBuf::from("/opt/bin/herdr"));
    assert_eq!(context.session_identity, "/tmp/herdr.sock");
}

#[test]
fn action_context_rejects_missing_required_values() {
    assert!(ActionContext::from_values("{}", "/tmp/state", "herdr", "/tmp/socket").is_err());
    assert!(ActionContext::from_values(
        r#"{"focused_pane_id":"w1:p1"}"#,
        "",
        "herdr",
        "/tmp/socket",
    )
    .is_err());
}

#[test]
fn plugin_panes_expose_only_opaque_state_ids() {
    let capture = annotation_popup_args(
        "abc123",
        &PopupSize {
            width: "70%".into(),
            height: "90%".into(),
        },
    );
    let review = review_popup_args(
        "def456",
        &PopupSize {
            width: "70%".into(),
            height: "85%".into(),
        },
    );

    assert_eq!(capture[0..3], ["plugin", "pane", "open"]);
    assert!(capture
        .windows(2)
        .any(|pair| pair == ["--placement", "popup"]));
    assert!(capture.windows(2).any(|pair| pair == ["--width", "70%"]));
    assert!(capture.windows(2).any(|pair| pair == ["--height", "90%"]));
    assert!(capture
        .iter()
        .any(|arg| arg == "HERDR_COMMENTS_RUN_ID=abc123"));
    assert!(review.windows(2).any(|pair| pair == ["--width", "70%"]));
    assert!(review.windows(2).any(|pair| pair == ["--height", "85%"]));
    assert!(review
        .iter()
        .any(|arg| arg == "HERDR_COMMENTS_REVIEW_ID=def456"));
}

#[test]
fn client_width_uses_the_full_layout_for_the_focused_pane() {
    assert_eq!(
        pane_layout_args("w1:p2"),
        ["pane", "layout", "--pane", "w1:p2"]
    );
}

#[test]
fn client_width_reads_the_layout_right_edge_from_herdr() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("herdr");
    std::fs::write(
        &bin,
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"layout\":{\"area\":{\"x\":30,\"width\":482}}}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(
        CliHerdr::new(bin, "/unused").client_width("w1:p2").unwrap(),
        512
    );
}

#[test]
fn stale_handoff_binary_uses_the_installed_sibling() {
    let temp = tempdir().unwrap();
    let installed = temp.path().join("herdr");
    std::fs::write(
        &installed,
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"layout\":{\"area\":{\"x\":30,\"width\":482}}}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let stale = temp.path().join("herdr.fast-scroll-staging");

    assert_eq!(
        CliHerdr::new(stale, "/unused")
            .client_width("w1:p2")
            .unwrap(),
        512
    );
}

#[test]
fn pane_capture_requests_rendered_history_without_unwrapping() {
    assert_eq!(
        pane_read_args("w1:p2", "recent", "ansi", Some(1_000)),
        ["pane", "read", "w1:p2", "--source", "recent", "--format", "ansi", "--lines", "1000"]
    );
}

#[test]
fn pane_input_uses_the_bracketed_paste_endpoint_without_enter() {
    let request: Value =
        serde_json::from_str(&pane_send_input_request("req-1", "w1:p2", "first\nsecond").unwrap())
            .unwrap();

    assert_eq!(request["method"], "pane.send_input");
    assert_eq!(request["params"]["pane_id"], "w1:p2");
    assert_eq!(request["params"]["text"], "first\nsecond");
    assert_eq!(request["params"]["keys"], Value::Array(Vec::new()));
}

#[test]
fn pane_input_rejects_a_request_larger_than_herdr_accepts() {
    let text = "a".repeat(1024 * 1024);

    assert!(pane_send_input_request("req-1", "w1:p2", &text).is_err());
}

#[test]
fn pane_input_uses_one_request_and_one_response_on_the_runtime_socket() {
    let temp = tempdir().unwrap();
    let socket_path = temp.path().join("herdr.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        let response = serde_json::json!({
            "id": request["id"],
            "result": {"type": "ok"}
        });
        writeln!(reader.get_mut(), "{response}").unwrap();
        (line, request)
    });

    CliHerdr::new("/unused/herdr", &socket_path)
        .send_input("w1:p2", "first\nsecond")
        .unwrap();
    let (line, request) = server.join().unwrap();

    assert_eq!(line.matches('\n').count(), 1);
    assert_eq!(request["method"], "pane.send_input");
    assert_eq!(request["params"]["keys"], Value::Array(Vec::new()));
}

#[test]
fn herdr_failures_are_actionable() {
    assert_eq!(classify_failure("error: ui_busy"), FailureKind::UiBusy);
    assert_eq!(classify_failure("pane_not_found"), FailureKind::PaneMissing);
    assert_eq!(classify_failure("connection refused"), FailureKind::Command);
}

#[test]
fn manifest_declares_the_public_contract() {
    let manifest = include_str!("../herdr-plugin.toml");

    assert!(manifest.contains("id = \"shadowfax.comments\""));
    assert!(manifest.contains("min_herdr_version = \"0.7.5\""));
    assert!(manifest.contains("platforms = [\"macos\"]"));
    assert!(manifest.contains("id = \"capture\""));
    assert!(manifest.contains("id = \"review\""));
    assert!(manifest.contains("id = \"paste\""));
    assert!(manifest.contains("placement = \"popup\""));
}
