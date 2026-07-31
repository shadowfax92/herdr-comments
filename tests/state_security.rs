use std::os::unix::fs::{symlink, PermissionsExt};

use herdr_comments::format::{format_collection, format_comment, preview_lines};
use herdr_comments::store::{scope_id, Store};
use tempfile::tempdir;

#[test]
fn comments_format_as_quote_then_note_in_capture_order() {
    let first = format_comment("first\r\nsecond", "because one").unwrap();
    let second = format_comment("third", "because two").unwrap();

    assert_eq!(first, "> first\n> second\n\nbecause one\n");
    assert_eq!(
        format_collection(&[first, second]),
        "> first\n> second\n\nbecause one\n\n> third\n\nbecause two\n"
    );
}

#[test]
fn preview_is_bounded_and_marks_truncation() {
    let preview = preview_lines("one\ntwo\nthree\nfour\nfive", 4, 2_000);

    assert_eq!(
        preview,
        vec![
            "> one",
            "> two",
            "> three",
            "> four",
            "> ... preview truncated"
        ]
    );
}

#[test]
fn state_is_isolated_by_session_and_pane() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let pane_a = scope_id("/tmp/session-a.sock", "w1:p1");
    let pane_b = scope_id("/tmp/session-a.sock", "w1:p2");
    let other_session = scope_id("/tmp/session-b.sock", "w1:p1");

    store.add_comment(&pane_a, "alpha", "note a").unwrap();
    store.add_comment(&pane_b, "beta", "note b").unwrap();
    store
        .add_comment(&other_session, "gamma", "note c")
        .unwrap();

    assert_eq!(store.list_comments(&pane_a).unwrap().len(), 1);
    assert_eq!(
        store.list_comments(&pane_a).unwrap()[0].source_text,
        "alpha"
    );
    assert_eq!(store.list_comments(&pane_b).unwrap()[0].source_text, "beta");
    assert_eq!(
        store.list_comments(&other_session).unwrap()[0].source_text,
        "gamma"
    );
    assert!(!pane_a.contains("session-a") && !pane_a.contains("w1:p1"));
}

#[test]
fn exact_cleanup_preserves_later_records() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let scope = scope_id("socket", "w1:p1");
    let first = store.add_comment(&scope, "one", "note").unwrap();
    let second = store.add_comment(&scope, "two", "note").unwrap();
    let later = store.add_comment(&scope, "three", "note").unwrap();

    store
        .delete_comments(&scope, &[first.id.clone(), second.id.clone()])
        .unwrap();

    let remaining = store.list_comments(&scope).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, later.id);
}

#[test]
fn state_directories_and_files_are_private() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("state");
    let store = Store::open(&root).unwrap();
    let scope = scope_id("socket", "w1:p1");
    let comment = store.add_comment(&scope, "source", "note").unwrap();

    let root_mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
    let file_mode = std::fs::metadata(store.comment_path(&scope, &comment.id))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(root_mode, 0o700);
    assert_eq!(file_mode, 0o600);
}

#[test]
fn symlink_roots_and_invalid_ids_are_rejected() {
    let temp = tempdir().unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let linked = temp.path().join("linked");
    symlink(&real, &linked).unwrap();

    assert!(Store::open(&linked).is_err());

    let store = Store::open(temp.path().join("state")).unwrap();
    assert!(store.load_draft("../outside").is_err());
}

#[test]
fn drafts_keep_originating_context_and_use_opaque_ids() {
    let temp = tempdir().unwrap();
    let store = Store::open(temp.path().join("state")).unwrap();
    let scope = scope_id("socket", "w4:p7");
    let draft = store.create_draft("copied text", "w4:p7", &scope).unwrap();
    let loaded = store.load_draft(&draft.id).unwrap();

    assert_eq!(loaded.source_text, "copied text");
    assert_eq!(loaded.pane_id, "w4:p7");
    assert_eq!(loaded.scope, scope);
    assert_eq!(loaded.id.len(), 32);
    assert!(loaded.id.chars().all(|ch| ch.is_ascii_hexdigit()));
}
