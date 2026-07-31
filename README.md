# Herdr Comments

Comment on copied terminal text, insert one comment immediately, or collect several comments for a Neovim review.

## Workflow

1. Press `Alt-w`, then `v`, move to select, and press `y` to copy.
2. Press `Alt-c` and write a one-line comment.
3. Choose what happens next:

| Key | Result |
| --- | --- |
| `Enter` | Insert this comment into the originating pane |
| `Option-Enter` or `Ctrl-Enter` | Collect it for that pane |
| `Ctrl-p` | Insert the collected comments plus this one |
| `Esc` | Discard this capture |

Press `Alt-Shift-c` to review the collected comments in Neovim. Use `:wq` to insert the edited document or `:q` to cancel and preserve the collection.

Text is inserted without `Enter`, so the destination program does not submit it. Output is quote-first Markdown:

```markdown
> selected terminal text

your comment
```

Collections are isolated by Herdr session and originating pane. A review removes only the comments in its snapshot, so comments collected while Neovim is open remain available.

## Install

Herdr 0.7.5 or newer, Neovim, and the Rust toolchain are required on macOS.

```sh
cd /Users/shadowfax/code/side-projects/herdr-custom-plugins/my/herdr-comments
herdr plugin link . --enabled
```

Add the bindings to `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "alt+c"
type = "plugin_action"
command = "shadowfax.comments.capture"
description = "Comment on copied terminal text"

[[keys.command]]
key = "alt+shift+c"
type = "plugin_action"
command = "shadowfax.comments.review"
description = "Review collected comments"
```

Then reload Herdr:

```sh
herdr server reload-config
```

The linked plugin uses this checkout. Rebuild after updating it with `cargo build --release --locked`.

## State

Herdr Comments stores private files under its Herdr state directory. Drafts and interrupted review sessions expire after 24 hours; collected comments remain until inserted. Set `HERDR_COMMENTS_NVIM` to use a Neovim executable other than `nvim`.

## Develop

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release --locked
```
