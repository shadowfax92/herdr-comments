<div align="center">

# 💬 Herdr Comments

**Select terminal history, attach comments, and review the result in Neovim.**

*Native popups for frozen Rust capture and a final Neovim edit.*

[![Herdr 0.7.5+](https://img.shields.io/badge/Herdr-0.7.5%2B-6c71c4)](https://herdr.dev)
[![Rust](https://img.shields.io/badge/built%20with-Rust-b7410e)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

Herdr Comments turns passages from a terminal pane into quote-first Markdown for an agent prompt. The capture phase stays inside a fast Rust interface; Neovim opens only when the complete collection is ready for review.

- **Frozen terminal history** — browse a stable snapshot while the source pane continues running.
- **Copy-mode navigation** — scroll and select with familiar vi keys.
- **Multiline comments** — attach a full note to each selected passage.
- **Per-pane collections** — every source pane and Herdr session has an isolated draft.
- **Responsive popups** — widths follow the active Herdr client, including a narrower ultrawide profile.
- **Neovim review** — reorder or rewrite the assembled Markdown, then save it as a ready draft.
- **Explicit paste** — a separate shortcut bracketed-pastes the ready draft with no follow-up keys, so nothing is submitted automatically.
- **Private local state** — snapshots, comments, reviews, and ready drafts use owner-only storage.

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

[[keys.command]]
key = "alt+shift+p"
type = "plugin_action"
command = "shadowfax.comments.paste"
description = "Paste saved comment review"
```

Reload Herdr:

```sh
herdr server reload-config
```

The first action also creates the plugin's `config.yaml`. Find it with:

```sh
herdr plugin config-dir shadowfax.comments
```

Popup profiles follow the same client-width model as Herdr Scratch. Profiles are checked in order; the first match wins.

```yaml
popups:
  capture: { width: "90%", height: "90%" }
  review: { width: "90%", height: "85%" }

profiles:
  - name: laptop
    match: { max_client_width: 310 }
    popups:
      capture: { width: "95%", height: "90%" }
      review: { width: "95%", height: "85%" }

  - name: 2-3-ultrawide
    match: { max_client_width: 350 }
    popups:
      capture: { width: "90%", height: "90%" }
      review: { width: "90%", height: "85%" }

  - name: full-ultrawide
    match: { min_client_width: 400 }
    popups:
      capture: { width: "70%", height: "90%" }
      review: { width: "70%", height: "85%" }
```

Widths from 351 through 399 cells use the 90% default. Changes are read on every action and do not require a Herdr reload.

## Workflow

Press `Alt-c` in a pane. Herdr Comments captures its rendered history and opens a centered native popup at the size selected by the active profile. The source remains visible around its edges.

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
| `q` | Finish annotating and close the popup |

### Write a comment

After `c`, the selected passage remains visible above a multiline Rust editor.

| Key | Result |
| --- | --- |
| `Enter` | Collect the comment and return to the snapshot |
| `Alt-Enter` | Insert a newline |
| `Esc` | Discard this comment |

Repeat selection and capture as many times as needed. Each saved entry is persisted immediately.

### Review in Neovim

Close the annotation popup with `q`, then press `Alt-Shift-c` in the source pane. Native popups receive their keys before Herdr's global bindings, so popup-local input cannot accidentally trigger this shortcut.

The Neovim popup contains only the assembled Markdown:

- `:wq` or `ZZ` saves the edited Markdown as this pane's ready draft and closes Neovim. It does not paste.
- `q` or `:qa!` cancels review. The collected comments and any previously saved ready draft remain intact.

Press `Alt-Shift-p` in the source pane when the draft is ready. That pastes it without sending `Enter`, then removes the pasted draft and only the comments included in it.

Herdr Comments calls Herdr's `pane.send_input` socket method with an empty key list. In applications that support bracketed paste, including the supported agent prompts, embedded newlines remain one unsubmitted paste. Inspect it and submit it yourself.

## Output

Every selection becomes a Markdown quote followed by its comment:

```markdown
> expected field `name`
> found field `title`

Can we support `title` as an alias instead?
```

Entries retain capture order and are separated by a blank line.

## Snapshot behavior

The public Herdr API hard-caps each pane read at 1,000 rendered rows. Herdr Comments captures both plain text and ANSI output: ANSI preserves the source styling, while plain text provides deterministic selections.

The popup owns its scrolling and visual selection. It does not preserve Herdr's native Copy mode. When the source viewport is older than the available recent-history window, the exact visible viewport is retained and the footer reports `history limited`.

## State and recovery

State is scoped by Herdr session and source pane under `HERDR_PLUGIN_STATE_DIR`.

- Saved comments remain when the annotation popup closes.
- Saving Neovim creates or replaces a persistent ready draft; cancelling preserves the previous one.
- Successful paste removes the ready draft and only the comments included in it.
- A failed paste preserves the ready draft and collection for retry.
- Interrupted annotation and Neovim sessions expire after 24 hours; collected comments and ready drafts do not.
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
