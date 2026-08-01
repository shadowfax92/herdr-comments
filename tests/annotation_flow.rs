use std::sync::Mutex;

use anyhow::{bail, Result};
use herdr_comments::annotation::{review_target, AnnotationService, AnnotationStart};
use herdr_comments::context::ActionContext;
use herdr_comments::herdr::{ClosePaneResult, HerdrClient};
use herdr_comments::model::PaneSnapshot;
use herdr_comments::store::{scope_id, session_key, Store};
use tempfile::tempdir;

struct RecordingHerdr {
    snapshot: PaneSnapshot,
    opened: Mutex<Vec<String>>,
    closed: Mutex<Vec<String>>,
    fail_open: Mutex<bool>,
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
            opened: Mutex::new(Vec::new()),
            closed: Mutex::new(Vec::new()),
            fail_open: Mutex::new(false),
        }
    }
}

impl HerdrClient for RecordingHerdr {
    fn capture_snapshot(&self, _pane_id: &str) -> Result<PaneSnapshot> {
        Ok(self.snapshot.clone())
    }

    fn open_annotation(&self, run_id: &str) -> Result<String> {
        if *self.fail_open.lock().unwrap() {
            bail!("injected open failure");
        }
        self.opened.lock().unwrap().push(run_id.to_owned());
        Ok("w1:p9".into())
    }

    fn close_plugin_pane(&self, pane_id: &str) -> Result<ClosePaneResult> {
        self.closed.lock().unwrap().push(pane_id.to_owned());
        Ok(ClosePaneResult::Closed)
    }

    fn open_review(&self, _review_id: &str) -> Result<()> {
        Ok(())
    }

    fn send_input(&self, _pane_id: &str, _text: &str) -> Result<()> {
        Ok(())
    }

    fn notify(&self, _title: &str, _body: &str) -> Result<()> {
        Ok(())
    }
}

#[test]
fn launch_persists_the_snapshot_before_opening_an_overlay() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    let scope = scope_id("socket", "w1:p1");
    let session_key = session_key("socket");

    let AnnotationStart::Opened(run) = AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope, &session_key)
        .unwrap()
    else {
        panic!("expected an opened annotation");
    };

    assert_eq!(herdr.opened.lock().unwrap().as_slice(), [run.id.as_str()]);
    assert_eq!(run.pane_id, "w1:p1");
    assert_eq!(run.overlay_pane_id.as_deref(), Some("w1:p9"));
    assert_eq!(store.load_annotation(&run.id).unwrap(), *run);
    assert_eq!(
        store
            .active_annotation(&session_key)
            .unwrap()
            .unwrap()
            .overlay_pane_id,
        "w1:p9"
    );
}

#[test]
fn invoking_capture_again_closes_the_active_annotation() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    let scope = scope_id("socket", "w1:p1");
    let session_key = session_key("socket");
    let AnnotationStart::Opened(run) = AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope, &session_key)
        .unwrap()
    else {
        panic!("expected an opened annotation");
    };

    assert_eq!(
        AnnotationService::new(&store, &herdr)
            .start("w1:p1", &scope, &session_key)
            .unwrap(),
        AnnotationStart::Closed
    );
    assert_eq!(herdr.closed.lock().unwrap().as_slice(), ["w1:p9"]);
    assert!(store.load_annotation(&run.id).is_err());
    assert!(store.active_annotation(&session_key).unwrap().is_none());
}

#[test]
fn active_overlays_are_isolated_between_herdr_sessions() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    let session_a = session_key("socket-a");
    let session_b = session_key("socket-b");

    AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope_id("socket-a", "w1:p1"), &session_a)
        .unwrap();
    AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope_id("socket-b", "w1:p1"), &session_b)
        .unwrap();

    assert!(herdr.closed.lock().unwrap().is_empty());
    assert!(store.active_annotation(&session_a).unwrap().is_some());
    assert!(store.active_annotation(&session_b).unwrap().is_some());
}

#[test]
fn failed_overlay_open_removes_the_private_snapshot() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    *herdr.fail_open.lock().unwrap() = true;
    let scope = scope_id("socket", "w1:p1");
    let session_key = session_key("socket");

    assert!(AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope, &session_key)
        .is_err());
    assert!(std::fs::read_dir(temp.path().join("state/runs"))
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn review_shortcut_on_the_overlay_resolves_the_source_pane() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::new();
    let scope = scope_id("socket", "w1:p1");
    let session_key = session_key("socket");
    let AnnotationStart::Opened(run) = AnnotationService::new(&store, &herdr)
        .start("w1:p1", &scope, &session_key)
        .unwrap()
    else {
        panic!("expected an opened annotation");
    };
    let context = ActionContext::from_values(
        r#"{"focused_pane_id":"w1:p9"}"#,
        temp.path().join("state").to_str().unwrap(),
        "/opt/bin/herdr",
        "socket",
    )
    .unwrap();

    let target = review_target(&store, &context).unwrap();

    assert_eq!(target.pane_id, "w1:p1");
    assert_eq!(target.scope, scope);
    assert_eq!(target.annotation_run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(target.overlay_pane_id.as_deref(), Some("w1:p9"));
}
