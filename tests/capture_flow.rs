use std::sync::Mutex;

use anyhow::{bail, Result};
use herdr_comments::capture::{CaptureService, Completion, CompletionResult};
use herdr_comments::clipboard::acquire_with;
use herdr_comments::herdr::HerdrClient;
use herdr_comments::store::{scope_id, Store};
use tempfile::tempdir;

#[derive(Default)]
struct RecordingHerdr {
    opened: Mutex<Vec<String>>,
    sent: Mutex<Vec<(String, String)>>,
    notifications: Mutex<Vec<(String, String)>>,
    fail_send: Mutex<bool>,
}

impl HerdrClient for RecordingHerdr {
    fn open_capture(&self, draft_id: &str) -> Result<()> {
        self.opened.lock().unwrap().push(draft_id.to_owned());
        Ok(())
    }

    fn open_review(&self, _review_id: &str) -> Result<()> {
        Ok(())
    }

    fn send_text(&self, pane_id: &str, text: &str) -> Result<()> {
        if *self.fail_send.lock().unwrap() {
            bail!("injected send failure");
        }
        self.sent
            .lock()
            .unwrap()
            .push((pane_id.to_owned(), text.to_owned()));
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) -> Result<()> {
        self.notifications
            .lock()
            .unwrap()
            .push((title.to_owned(), body.to_owned()));
        Ok(())
    }
}

#[test]
fn clipboard_validation_happens_before_draft_creation() {
    assert_eq!(
        acquire_with(|| Ok(b"one\r\ntwo".to_vec())).unwrap(),
        "one\ntwo"
    );
    assert!(acquire_with(|| Ok(Vec::new())).is_err());
    assert!(acquire_with(|| Ok(b"unsafe\x1b[31m".to_vec())).is_err());
    assert!(acquire_with(|| Ok(vec![b'x'; 200 * 1024 + 1])).is_err());
    assert!(acquire_with(|| Ok(vec![0xff])).is_err());
}

#[test]
fn capture_persists_before_opening_with_an_opaque_id() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = CaptureService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");

    let started = service.start("copied text", "w1:p1", &scope).unwrap();

    let opened = herdr.opened.lock().unwrap();
    assert_eq!(opened.as_slice(), [started.draft.id.as_str()]);
    assert_eq!(store.load_draft(&started.draft.id).unwrap(), started.draft);
    assert_eq!(started.collected_count, 0);
}

#[test]
fn enter_inserts_one_comment_without_submitting() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = CaptureService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    let started = service.start("selected", "w1:p1", &scope).unwrap();

    let result = service
        .complete(&started.draft.id, "explain this", Completion::Insert)
        .unwrap();

    assert_eq!(result, CompletionResult::Inserted { count: 1 });
    assert_eq!(
        herdr.sent.lock().unwrap().as_slice(),
        [("w1:p1".into(), "> selected\n\nexplain this\n".into())]
    );
    assert!(store.load_draft(&started.draft.id).is_err());
    assert!(store.list_comments(&scope).unwrap().is_empty());
}

#[test]
fn option_enter_collects_and_ctrl_p_pastes_collection_plus_current() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = CaptureService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    let first = service.start("first", "w1:p1", &scope).unwrap();

    let collected = service
        .complete(&first.draft.id, "first note", Completion::Collect)
        .unwrap();
    assert_eq!(collected, CompletionResult::Collected { count: 1 });

    let second = service.start("second", "w1:p1", &scope).unwrap();
    assert_eq!(second.collected_count, 1);
    let pasted = service
        .complete(&second.draft.id, "second note", Completion::PasteAll)
        .unwrap();

    assert_eq!(pasted, CompletionResult::Inserted { count: 2 });
    assert_eq!(
        herdr.sent.lock().unwrap()[0].1,
        "> first\n\nfirst note\n\n> second\n\nsecond note\n"
    );
    assert!(store.list_comments(&scope).unwrap().is_empty());
    assert!(store.load_draft(&second.draft.id).is_err());
}

#[test]
fn escape_discards_only_the_transient_draft() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = CaptureService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "kept", "note").unwrap();
    let started = service.start("discarded", "w1:p1", &scope).unwrap();

    let result = service
        .complete(&started.draft.id, "", Completion::Cancel)
        .unwrap();

    assert_eq!(result, CompletionResult::Cancelled);
    assert!(store.load_draft(&started.draft.id).is_err());
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert!(herdr.sent.lock().unwrap().is_empty());
}

#[test]
fn send_failure_preserves_the_draft_and_collection() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = CaptureService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "kept", "note").unwrap();
    let started = service.start("current", "w1:p1", &scope).unwrap();
    *herdr.fail_send.lock().unwrap() = true;

    assert!(service
        .complete(&started.draft.id, "current note", Completion::PasteAll)
        .is_err());

    assert!(store.load_draft(&started.draft.id).is_ok());
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
}
