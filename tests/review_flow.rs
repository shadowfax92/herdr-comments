use std::sync::Mutex;

use anyhow::{bail, Result};
use herdr_comments::annotation::ReviewTarget;
use herdr_comments::config::PopupSize;
use herdr_comments::herdr::HerdrClient;
use herdr_comments::model::{PaneSnapshot, ReviewSession};
use herdr_comments::review::{PasteResult, ReviewResult, ReviewService, ReviewStart};
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
    fn capture_snapshot(&self, _pane_id: &str) -> Result<PaneSnapshot> {
        unreachable!()
    }

    fn open_annotation(&self, _run_id: &str, _popup: &PopupSize) -> Result<()> {
        unreachable!()
    }

    fn open_inline_annotation(&self, _run_id: &str, _popup: &PopupSize) -> Result<()> {
        unreachable!()
    }

    fn open_review(&self, review_id: &str, _popup: &PopupSize) -> Result<()> {
        self.opened.lock().unwrap().push(review_id.to_owned());
        Ok(())
    }

    fn send_input(&self, pane_id: &str, text: &str) -> Result<()> {
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

fn target(scope: &str) -> ReviewTarget {
    ReviewTarget {
        pane_id: "w1:p1".into(),
        scope: scope.into(),
    }
}

fn popup() -> PopupSize {
    PopupSize {
        width: "70%".into(),
        height: "85%".into(),
    }
}

fn opened_review(start: ReviewStart) -> ReviewSession {
    let ReviewStart::Opened(review) = start else {
        panic!("expected an open review");
    };
    review
}

fn save_review(
    store: &Store,
    service: &ReviewService<'_, RecordingHerdr>,
    scope: &str,
    markdown: &str,
) -> ReviewSession {
    let review = opened_review(service.start(&target(scope), &popup()).unwrap());
    std::fs::write(store.review_markdown_path(&review.id).unwrap(), markdown).unwrap();
    store.confirm_review(&review.id).unwrap();
    assert_eq!(
        service.finish(&review.id).unwrap(),
        ReviewResult::Saved {
            count: review.comment_ids.len()
        }
    );
    review
}

#[test]
fn empty_collection_notifies_without_opening() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");

    assert_eq!(
        service.start(&target(&scope), &popup()).unwrap(),
        ReviewStart::Empty
    );
    assert!(herdr.opened.lock().unwrap().is_empty());
    assert!(herdr.notifications.lock().unwrap()[0]
        .1
        .contains("No comments"));
}

#[test]
fn review_opens_an_exact_markdown_snapshot_with_an_opaque_id() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note one").unwrap();
    store.add_comment(&scope, "second", "note two").unwrap();

    let review = opened_review(service.start(&target(&scope), &popup()).unwrap());

    assert_eq!(
        herdr.opened.lock().unwrap().as_slice(),
        [review.id.as_str()]
    );
    assert_eq!(
        store.read_review_markdown(&review.id).unwrap(),
        "> first\n\nnote one\n\n> second\n\nnote two\n"
    );
    assert_eq!(review.comment_ids.len(), 2);
}

#[test]
fn saving_creates_a_ready_draft_without_pasting_or_clearing_comments() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note one").unwrap();
    store.add_comment(&scope, "second", "note two").unwrap();
    let edited = "> second\n\nrevised\n\n> first\n\nnote one\n";

    let review = save_review(&store, &service, &scope, edited);

    let ready = store.ready_review(&scope).unwrap().unwrap();
    assert_eq!(ready.markdown, edited);
    assert_eq!(ready.comment_ids, review.comment_ids);
    assert!(herdr.sent.lock().unwrap().is_empty());
    assert_eq!(store.list_comments(&scope).unwrap().len(), 2);
    assert!(store.load_review(&review.id).is_err());
}

#[test]
fn cancelling_review_preserves_comments_and_the_previous_ready_draft() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    save_review(&store, &service, &scope, "saved draft\n");
    let review = opened_review(service.start(&target(&scope), &popup()).unwrap());

    assert_eq!(service.finish(&review.id).unwrap(), ReviewResult::Cancelled);
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert_eq!(
        store.ready_review(&scope).unwrap().unwrap().markdown,
        "saved draft\n"
    );
    assert!(store.load_review(&review.id).is_err());
    assert!(herdr.sent.lock().unwrap().is_empty());
}

#[test]
fn saving_a_blank_review_preserves_the_previous_ready_draft() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    save_review(&store, &service, &scope, "saved draft\n");
    let review = opened_review(service.start(&target(&scope), &popup()).unwrap());
    std::fs::write(store.review_markdown_path(&review.id).unwrap(), "\n").unwrap();
    store.confirm_review(&review.id).unwrap();

    assert_eq!(service.finish(&review.id).unwrap(), ReviewResult::Cancelled);
    assert_eq!(
        store.ready_review(&scope).unwrap().unwrap().markdown,
        "saved draft\n"
    );
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert!(herdr.sent.lock().unwrap().is_empty());
}

#[test]
fn paste_without_comments_or_a_ready_draft_notifies_and_does_nothing() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");

    assert_eq!(
        service.paste_ready(&target(&scope)).unwrap(),
        PasteResult::Empty
    );
    assert!(herdr.sent.lock().unwrap().is_empty());
    assert!(herdr.notifications.lock().unwrap()[0]
        .1
        .contains("No collected comments"));
}

#[test]
fn paste_without_a_ready_draft_assembles_and_sends_collected_comments() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note one").unwrap();
    store.add_comment(&scope, "second", "note two").unwrap();

    assert_eq!(
        service.paste_ready(&target(&scope)).unwrap(),
        PasteResult::Pasted { count: 2 }
    );

    assert_eq!(
        herdr.sent.lock().unwrap().as_slice(),
        [(
            "w1:p1".into(),
            "> first\n\nnote one\n\n> second\n\nnote two\n".into()
        )]
    );
    assert!(store.list_comments(&scope).unwrap().is_empty());
    assert!(store.ready_review(&scope).unwrap().is_none());
}

#[test]
fn direct_paste_failure_preserves_collected_comments_for_retry() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    *herdr.fail_send.lock().unwrap() = true;

    assert!(service.paste_ready(&target(&scope)).is_err());
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert!(store.ready_review(&scope).unwrap().is_some());
}

#[test]
fn paste_sends_the_ready_draft_and_clears_only_its_comments() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    save_review(&store, &service, &scope, "edited draft\n");
    let later = store.add_comment(&scope, "later", "keep me").unwrap();

    assert_eq!(
        service.paste_ready(&target(&scope)).unwrap(),
        PasteResult::Pasted { count: 1 }
    );

    assert_eq!(
        herdr.sent.lock().unwrap().as_slice(),
        [("w1:p1".into(), "edited draft\n".into())]
    );
    assert!(store.ready_review(&scope).unwrap().is_none());
    let remaining = store.list_comments(&scope).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, later.id);
}

#[test]
fn send_failure_retains_the_ready_draft_and_collection_for_retry() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    save_review(&store, &service, &scope, "saved draft\n");
    *herdr.fail_send.lock().unwrap() = true;

    assert!(service.paste_ready(&target(&scope)).is_err());
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert_eq!(
        store.ready_review(&scope).unwrap().unwrap().markdown,
        "saved draft\n"
    );
}

#[test]
fn a_new_saved_review_replaces_the_previous_ready_draft() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    save_review(&store, &service, &scope, "first draft\n");
    store.add_comment(&scope, "second", "note").unwrap();

    save_review(&store, &service, &scope, "replacement draft\n");

    let ready = store.ready_review(&scope).unwrap().unwrap();
    assert_eq!(ready.markdown, "replacement draft\n");
    assert_eq!(ready.comment_ids.len(), 2);
}
