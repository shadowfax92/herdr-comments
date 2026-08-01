use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn wqa_saves_the_review_when_another_modified_buffer_has_no_name() {
    let Ok(version) = Command::new("nvim").arg("--version").output() else {
        return;
    };
    if !version.status.success() {
        return;
    }

    let temp = tempdir().unwrap();
    let review = temp.path().join("review.md");
    let confirm = temp.path().join("confirm");
    let marker = temp.path().join("confirmed");
    std::fs::write(&review, "review\n").unwrap();
    std::fs::write(
        &confirm,
        "#!/bin/sh\nset -eu\n[ \"$1\" = confirm-review ]\n[ \"$2\" = --id ]\nprintf '%s\\n' \"$3\" > \"$HERDR_COMMENTS_CONFIRM_MARKER\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&confirm, std::fs::Permissions::from_mode(0o700)).unwrap();

    let output = Command::new("nvim")
        .args(["--headless", "-u", "NONE", "-n"])
        .args(["-c", "lua dofile(vim.env.HERDR_COMMENTS_REVIEW_LUA)"])
        .args([
            "-c",
            "lua local b=vim.api.nvim_create_buf(true, false); vim.api.nvim_buf_set_lines(b, 0, -1, false, {'unsaved'}); vim.bo[b].modified=true",
        ])
        .args([
            "-c",
            "lua local keys=vim.api.nvim_replace_termcodes(':wqa<CR>', true, false, true); vim.api.nvim_feedkeys(keys, 'x', false)",
        ])
        .args(["-c", "qa!"])
        .arg(&review)
        .env("HERDR_COMMENTS_REVIEW_ID", "review-id")
        .env("HERDR_COMMENTS_BIN", &confirm)
        .env(
            "HERDR_COMMENTS_REVIEW_LUA",
            concat!(env!("CARGO_MANIFEST_DIR"), "/nvim/review.lua"),
        )
        .env("HERDR_COMMENTS_CONFIRM_MARKER", &marker)
        .output()
        .unwrap();

    assert!(
        marker.is_file(),
        "Neovim did not confirm the review: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "review-id\n");
}
