use std::sync::Mutex;

use anyhow::{bail, Result};
use herdr_comments::annotation::{review_target, AnnotationService};
use herdr_comments::config::PopupSize;
use herdr_comments::context::ActionContext;
use herdr_comments::herdr::HerdrClient;
use herdr_comments::model::PaneSnapshot;
use herdr_comments::store::{scope_id, Store};
use tempfile::tempdir;

struct RecordingHerdr {
    snapshot: PaneSnapshot,
    captures: Mutex<usize>,
    opened: Mutex<Vec<String>>,
    opened_inline: Mutex<Vec<String>>,
    fail_open: Mutex<bool>,
    fail_inline_open: Mutex<bool>,
}

impl RecordingHerdr {
    fn new() -> Self {
        Self {
            snapshot: PaneSnapshot {
                text: "one\ntwo\nthree".into(),
                ansi: "one\n\u{1b}[31mtwo\u{1b}[0m\nthree".into(),
                initial_top: 0,
                viewport_rows: 3,
                history_limited: false,
            },
            captures: Mutex::new(0),
            opened: Mutex::new(Vec::new()),
            opened_inline: Mutex::new(Vec::new()),
            fail_open: Mutex::new(false),
            fail_inline_open: Mutex::new(false),
        }
    }
}

impl HerdrClient for RecordingHerdr {
    fn capture_snapshot(&self, _pane_id: &str) -> Result<PaneSnapshot> {
        *self.captures.lock().unwrap() += 1;
        Ok(self.snapshot.clone())
    }

    fn open_annotation(&self, run_id: &str, _popup: &PopupSize) -> Result<()> {
        if *self.fail_open.lock().unwrap() {
            bail!("injected open failure");
        }
        self.opened.lock().unwrap().push(run_id.to_owned());
        Ok(())
    }

    fn open_inline_annotation(&self, run_id: &str, _popup: &PopupSize) -> Result<()> {
        if *self.fail_inline_open.lock().unwrap() {
            bail!("injected inline open failure");
        }
        self.opened_inline.lock().unwrap().push(run_id.to_owned());
        Ok(())
    }

    fn open_review(&self, _review_id: &str, _popup: &PopupSize) -> Result<()> {
        Ok(())
    }

    fn send_input(&self, _pane_id: &str, _text: &str) -> Result<()> {
        Ok(())
    }

    fn notify(&self, _title: &str, _body: &str) -> Result<()> {
        Ok(())
    }
}

fn popup() -> PopupSize {
    PopupSize {
        width: "70%".into(),
        height: "90%".into(),
    }
}

#[test]
fn launch_persists_the_snapshot_before_opening_a_popup() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    let scope = scope_id("socket", "w1:p1");

    let run = AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope, &popup())
        .unwrap();

    assert_eq!(herdr.opened.lock().unwrap().as_slice(), [run.id.as_str()]);
    assert_eq!(run.pane_id, "w1:p1");
    assert_eq!(store.load_annotation(&run.id).unwrap(), run);
}

#[test]
fn failed_popup_open_removes_the_private_snapshot() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    *herdr.fail_open.lock().unwrap() = true;
    let scope = scope_id("socket", "w1:p1");

    assert!(AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope, &popup())
        .is_err());
    assert!(std::fs::read_dir(temp.path().join("state/runs"))
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn inline_launch_persists_the_native_selection_without_capturing_the_pane() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    let scope = scope_id("socket", "w1:p1");

    let run = AnnotationService::new(&store, &herdr)
        .start_inline("w1:p1", &scope, "first\r\nsecond", &popup())
        .unwrap();

    assert_eq!(*herdr.captures.lock().unwrap(), 0);
    assert_eq!(
        herdr.opened_inline.lock().unwrap().as_slice(),
        [run.id.as_str()]
    );
    assert_eq!(run.snapshot.text, "first\nsecond");
    assert_eq!(run.snapshot.ansi, "first\nsecond");
    assert_eq!(run.snapshot.viewport_rows, 2);
    assert_eq!(store.load_annotation(&run.id).unwrap(), run);
}

#[test]
fn failed_inline_popup_open_removes_the_private_selection() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    *herdr.fail_inline_open.lock().unwrap() = true;
    let scope = scope_id("socket", "w1:p1");

    assert!(AnnotationService::new(&store, &herdr)
        .start_inline("w1:p1", &scope, "selected", &popup())
        .is_err());
    assert!(std::fs::read_dir(temp.path().join("state/runs"))
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn inline_launch_rejects_an_empty_native_selection() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    let scope = scope_id("socket", "w1:p1");

    let error = AnnotationService::new(&store, &herdr)
        .start_inline("w1:p1", &scope, " \r\n ", &popup())
        .unwrap_err();

    assert!(error.to_string().contains("selected text is empty"));
    assert_eq!(*herdr.captures.lock().unwrap(), 0);
    assert!(herdr.opened_inline.lock().unwrap().is_empty());
}

#[test]
fn review_target_uses_the_focused_source_pane() {
    let context = ActionContext::from_values(
        r#"{"focused_pane_id":"w1:p1"}"#,
        "/tmp/comments-state",
        "/opt/bin/herdr",
        "socket",
    )
    .unwrap();

    let target = review_target(&context);

    assert_eq!(target.pane_id, "w1:p1");
    assert_eq!(target.scope, scope_id("socket", "w1:p1"));
}
