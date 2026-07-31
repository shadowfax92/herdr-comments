use std::sync::Mutex;

use anyhow::{bail, Result};
use herdr_comments::herdr::HerdrClient;
use herdr_comments::review::{ReviewResult, ReviewService, ReviewStart};
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
    fn open_capture(&self, _draft_id: &str) -> Result<()> {
        Ok(())
    }

    fn open_review(&self, review_id: &str) -> Result<()> {
        self.opened.lock().unwrap().push(review_id.to_owned());
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
fn empty_collection_notifies_without_opening() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");

    assert_eq!(service.start("w1:p1", &scope).unwrap(), ReviewStart::Empty);
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

    let ReviewStart::Opened(review) = service.start("w1:p1", &scope).unwrap() else {
        panic!("expected an open review");
    };

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
fn saved_review_inserts_exact_edits_and_clears_the_snapshot() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note one").unwrap();
    store.add_comment(&scope, "second", "note two").unwrap();
    let ReviewStart::Opened(review) = service.start("w1:p1", &scope).unwrap() else {
        panic!("expected an open review");
    };
    let edited = "> second\n\nrevised\n\n> first\n\nnote one\n";
    std::fs::write(store.review_markdown_path(&review.id).unwrap(), edited).unwrap();
    store.confirm_review(&review.id).unwrap();

    let result = service.complete(&review.id).unwrap();

    assert_eq!(result, ReviewResult::Inserted { count: 2 });
    assert_eq!(
        herdr.sent.lock().unwrap()[0],
        ("w1:p1".into(), edited.into())
    );
    assert!(store.list_comments(&scope).unwrap().is_empty());
    assert!(store.load_review(&review.id).is_err());
}

#[test]
fn quitting_without_write_cancels_and_preserves_collection() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    let ReviewStart::Opened(review) = service.start("w1:p1", &scope).unwrap() else {
        panic!("expected an open review");
    };

    assert_eq!(
        service.complete(&review.id).unwrap(),
        ReviewResult::Cancelled
    );
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert!(store.load_review(&review.id).is_err());
    assert!(herdr.sent.lock().unwrap().is_empty());
}

#[test]
fn saved_blank_review_preserves_collection_without_inserting() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    let ReviewStart::Opened(review) = service.start("w1:p1", &scope).unwrap() else {
        panic!("expected an open review");
    };
    std::fs::write(store.review_markdown_path(&review.id).unwrap(), "\n").unwrap();
    store.confirm_review(&review.id).unwrap();

    assert_eq!(
        service.complete(&review.id).unwrap(),
        ReviewResult::Cancelled
    );
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert!(herdr.sent.lock().unwrap().is_empty());
}

#[test]
fn send_failure_retains_review_and_collection_for_retry() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    let ReviewStart::Opened(review) = service.start("w1:p1", &scope).unwrap() else {
        panic!("expected an open review");
    };
    store.confirm_review(&review.id).unwrap();
    *herdr.fail_send.lock().unwrap() = true;

    assert!(service.complete(&review.id).is_err());
    assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    assert!(store.load_review(&review.id).is_ok());
}

#[test]
fn successful_review_deletes_only_the_snapshotted_comments() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let herdr = RecordingHerdr::default();
    let service = ReviewService::new(&store, &herdr);
    let scope = scope_id("socket", "w1:p1");
    store.add_comment(&scope, "first", "note").unwrap();
    let ReviewStart::Opened(review) = service.start("w1:p1", &scope).unwrap() else {
        panic!("expected an open review");
    };
    let later = store.add_comment(&scope, "later", "keep me").unwrap();
    store.confirm_review(&review.id).unwrap();

    service.complete(&review.id).unwrap();

    let remaining = store.list_comments(&scope).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, later.id);
}
