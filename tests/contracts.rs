use std::path::PathBuf;

use herdr_comments::context::ActionContext;
use herdr_comments::herdr::{
    capture_popup_args, classify_failure, paste_args, review_popup_args, FailureKind,
};

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
fn popup_arguments_expose_only_the_opaque_id() {
    let capture = capture_popup_args("abc123");
    let review = review_popup_args("def456");

    assert_eq!(capture[0..3], ["plugin", "pane", "open"]);
    assert!(capture.windows(2).any(|pair| pair == ["--width", "48%"]));
    assert!(capture.windows(2).any(|pair| pair == ["--height", "30%"]));
    assert!(capture
        .iter()
        .any(|arg| arg == "HERDR_COMMENTS_DRAFT_ID=abc123"));
    assert!(review.windows(2).any(|pair| pair == ["--width", "90%"]));
    assert!(review.windows(2).any(|pair| pair == ["--height", "85%"]));
    assert!(review
        .iter()
        .any(|arg| arg == "HERDR_COMMENTS_REVIEW_ID=def456"));
}

#[test]
fn pane_text_is_pasted_with_multiline_content_and_without_enter() {
    let text = "> first\n>\n> second\n\nMy comment";
    let args = paste_args("w1:p2", text);

    assert_eq!(args, ["pane", "paste", "w1:p2", text]);
    assert!(!args.iter().any(|arg| arg == "enter" || arg == "run"));
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
    assert!(manifest.contains("placement = \"popup\""));
}
