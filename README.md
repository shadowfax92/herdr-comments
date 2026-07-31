<div align="center">

# 💬 Herdr Comments

**Annotate terminal output and send the context back without leaving Herdr.**

*Insert one thought now, or collect a review round and polish it in Neovim.*

[![Herdr 0.7.5+](https://img.shields.io/badge/Herdr-0.7.5%2B-6c71c4)](https://herdr.dev)
[![Rust](https://img.shields.io/badge/built%20with-Rust-b7410e)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

Herdr Comments turns copied terminal text into quote-first Markdown for your next agent prompt. Capture a single comment, accumulate several comments per pane, or open the whole collection in Neovim for a final editing pass.

- **One-shot comments** — paste one annotation into the originating pane immediately.
- **Draft-first review rounds** — collect comments without touching the pane, then review them together.
- **Exact source context** — every note stays attached to the terminal text that prompted it.
- **Safe insertion** — multiline Markdown is pasted without pressing Enter, so nothing is submitted for you.
- **Per-pane isolation** — collections are scoped to the Herdr session and source pane.
- **Local private state** — drafts and reviews stay in Herdr's plugin state directory with private permissions.

## Install

Requires macOS, [Herdr](https://herdr.dev) 0.7.5 or newer, a Rust toolchain, and Neovim for collection review.

```sh
herdr plugin install shadowfax92/herdr-comments
```

Add the actions to `~/.config/herdr/config.toml`:

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

Reload the running server:

```sh
herdr server reload-config
```

## Workflow

1. Enter Herdr Copy mode, select text with `v`, and copy it with `y`.
2. Press `Alt-c` and write a one-line comment.
3. Insert it, collect it, or paste the whole collection.

| Key | Result |
| --- | --- |
| `Enter` | Paste this comment into the originating pane |
| `Option-Enter` or `Ctrl-Enter` | Collect it for this pane without changing the pane |
| `Ctrl-p` | Paste the collected comments followed by this comment |
| `Esc` | Discard this capture |

Press `Alt-Shift-c` when you are ready to review a collection. Neovim opens the complete Markdown document:

- `:wq` confirms the edited document and pastes it into the originating pane.
- `:q` cancels the review and preserves the collection.

Herdr Comments never sends Enter. Review the resulting prompt and submit it yourself.

## Output

Each capture is rendered as quote-first Markdown:

```markdown
> selected terminal text

your comment
```

Multiple captures are separated by blank lines and retain their capture order. A review removes only the comments in its original snapshot, so new comments collected while Neovim is open remain available for the next round.

## State and recovery

Herdr provides the plugin's state directory. Herdr Comments stores drafts, collections, and review snapshots there using private directories and files.

- Interrupted drafts and review sessions expire after 24 hours.
- Collected comments remain until they are successfully inserted.
- Failed pane insertion preserves the draft or review for another attempt.
- `HERDR_COMMENTS_NVIM` can select a Neovim executable other than `nvim`.

## Local development

```sh
git clone https://github.com/shadowfax92/herdr-comments.git
cd herdr-comments
herdr plugin link .
```

Run the complete local gate:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

A linked plugin runs from this checkout. Rebuild the release binary after changing the code.

## License

[MIT](LICENSE)
