<div align="center">

# 💬 Herdr Comments

**Select terminal history, attach comments, and review the result in Neovim.**

*A frozen Rust annotation view for capture and a native popup for the final edit.*

[![Herdr 0.7.5+](https://img.shields.io/badge/Herdr-0.7.5%2B-6c71c4)](https://herdr.dev)
[![Rust](https://img.shields.io/badge/built%20with-Rust-b7410e)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

Herdr Comments turns passages from a terminal pane into quote-first Markdown for an agent prompt. The capture phase stays inside a fast Rust interface; Neovim opens only when the complete collection is ready for review.

- **Frozen terminal history** — browse a stable snapshot while the source pane continues running.
- **Copy-mode navigation** — scroll and select with familiar vi keys.
- **Multiline comments** — attach a full note to each selected passage.
- **Per-pane collections** — every source pane and Herdr session has an isolated draft.
- **Neovim review** — reorder or rewrite the assembled Markdown before insertion.
- **Safe insertion** — `pane.send_input` bracketed-pastes the review with no follow-up keys, so nothing is submitted automatically.
- **Private local state** — snapshots, comments, and reviews use owner-only storage.

## Install

Requires macOS, [Herdr](https://herdr.dev) 0.7.5 or newer, a Rust toolchain, and Neovim.

```sh
herdr plugin install shadowfax92/herdr-comments
```

Add the actions to `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "alt+c"
type = "plugin_action"
command = "shadowfax.comments.capture"
description = "Annotate pane history"

[[keys.command]]
key = "alt+shift+c"
type = "plugin_action"
command = "shadowfax.comments.review"
description = "Review collected comments"
```

Reload Herdr:

```sh
herdr server reload-config
```

## Workflow

Press `Alt-c` in a pane. Herdr Comments captures its rendered history and opens a temporary native overlay. Pressing `Alt-c` again toggles the overlay closed.

### Browse and select

| Key | Result |
| --- | --- |
| `h`, `j`, `k`, `l` or arrows | Move through the frozen snapshot |
| `Ctrl-u`, `Ctrl-d` | Move half a page |
| `PageUp`, `PageDown` | Move one page |
| `g`, `G` | Jump to the beginning or end |
| `v` | Begin character selection |
| `V` | Begin line selection |
| `c` | Comment on the active selection |
| `Esc` | Cancel a selection, or close from normal mode |
| `q` | Close the annotation overlay |

### Write a comment

After `c`, the selected passage remains visible above a multiline Rust editor.

| Key | Result |
| --- | --- |
| `Enter` | Insert a newline |
| `Ctrl-s` | Save the comment and return to the snapshot |
| `Esc` | Discard this comment |

Repeat selection and capture as many times as needed. Each saved entry is persisted immediately.

### Review in Neovim

Press `Alt-Shift-c` after collecting comments. The global shortcut recognizes the active annotation overlay and opens the collection belonging to its original source pane.

The Neovim popup contains only the assembled Markdown:

- `:wq` or `ZZ` confirms the edit, pastes it into the original pane, and closes the annotation overlay.
- `q` or `:qa!` cancels review and returns to the annotation overlay with the collection intact.

Herdr Comments calls Herdr's `pane.send_input` socket method with an empty key list. In applications that support bracketed paste, including the supported agent prompts, embedded newlines remain one unsubmitted paste. Review it and submit it yourself.

## Output

Every selection becomes a Markdown quote followed by its comment:

```markdown
> expected field `name`
> found field `title`

Can we support `title` as an alias instead?
```

Entries retain capture order and are separated by a blank line.

## Snapshot behavior

The public Herdr API exposes the current viewport and up to 1,000 recent rendered rows. Herdr Comments captures both plain text and ANSI output: ANSI preserves the backdrop, while plain text provides deterministic selections.

The overlay owns its scrolling and visual selection. It does not preserve Herdr's native Copy mode. When the source viewport is older than the available recent-history window, the exact visible viewport is retained and the footer reports `history limited`.

## State and recovery

State is scoped by Herdr session and source pane under `HERDR_PLUGIN_STATE_DIR`.

- Saved comments remain when the annotation overlay closes.
- Cancelling Neovim preserves the collection.
- Successful insertion removes only the comments included in that review.
- A failed insertion preserves the review and collection for recovery.
- Interrupted snapshots and reviews expire after 24 hours.
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

Linked plugins run from the checkout. Rebuild the release binary after code changes and reload Herdr's config after manifest or keybinding changes.

## License

MIT
