# egui_widgets

Reusable widgets for [egui](https://github.com/emilk/egui).

## Widgets

- `WheelBox` – numeric input box for all primitive integers (`i8`–`i128`, `u8`–`u128`,
  `isize`, `usize`) and floats (`f32`, `f64`).
  Click (or Tab) to focus, then:
  - `↑` / `↓` or the mouse wheel steps the value (one step per wheel notch; touchpads step
    at a steady pace),
  - `Q` / `W` halves / doubles the step, `R` resets it,
  - typing a number and pressing `Enter` (or moving the focus away) commits it, `Esc` discards it.

  The text is selected on focus and after each step, so typing replaces it. `Q`, `W` and `R`
  are shortcuts only and are never typed.

  `decimals` only limits what is shown, the value keeps its full precision. Stepping floats
  doesn't accumulate rounding errors: `0.5` stepped by `0.1` gives exactly `0.6`.

  ```rust
  ui.add(WheelBox::new(&mut count).range([0, 100]).prefix("n: "));
  // floats default to 3 decimals
  ui.add(WheelBox::new(&mut exposure).decimals(2).suffix(RichText::new("ms").weak()));
  ```

  `range` may be given in either order. Typed and stepped values are clamped to it, a value
  set by the application is shown as it is until the user changes it.

  `prefix` / `suffix` take egui [atoms](https://docs.rs/egui/latest/egui/struct.Atom.html)
  (text, `RichText`, images, or tuples of them) and stay visible while editing.
  If the box is too narrow, the suffix is hidden first, then the prefix, and only then is the
  value truncated.

  Like other egui widgets, the box gets its id from its position in the `Ui`. If widgets
  before it come and go while it's being edited, give it a stable id with `.id_salt(...)`.

  For screen readers the box is named by the text of its prefix and suffix, like
  `egui::DragValue`. Without them, name it with `.labelled_by(label.id)` on the response
  of a visible label next to it, or with `.accessible_name("...")`:

  ```rust
  let label = ui.label("Exposure");
  ui.add(WheelBox::new(&mut exposure).suffix("ms")).labelled_by(label.id);
  ```

## Example

An interactive demo lives in [`example/`](example/).

Native:

```sh
cargo run -p egui_widgets_example
```

Web (requires [Trunk](https://trunkrs.dev) and the `wasm32-unknown-unknown` target):

```sh
rustup target add wasm32-unknown-unknown
trunk serve   # http://127.0.0.1:8190
```

## Testing

```sh
cargo test
```

- Unit tests for the pure value logic (parsing, stepping, clamping, wheel handling) are next
  to the code in `src/`.
- Interaction tests in `tests/` drive the widgets headlessly with
  [`egui_kittest`](https://crates.io/crates/egui_kittest). The widgets are exposed to AccessKit
  as text inputs whose value is the displayed number, so tests can find them by value.

CI runs Clippy and the tests on Linux, Windows and macOS, and checks formatting, the docs,
the minimum supported Rust version (1.95) and a WebAssembly build of the demo, see
[`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## Changelog and releases

[`CHANGELOG.md`](CHANGELOG.md) is generated from committed Git history with
[git-cliff](https://git-cliff.org/), configured in [`cliff.toml`](cliff.toml).

Use [Conventional Commits](https://www.conventionalcommits.org/), for example
`feat(wheel-box): add a new option`, `fix(wheel-box): handle empty input`, or
`feat!: change the public API`. Breaking changes can also be marked with a
`BREAKING CHANGE:` footer describing how callers should migrate. When squash-merging
a PR, use its Conventional Commit title as the resulting commit message.

Features, fixes, performance improvements, documentation, refactors and reverts are
included. Routine `chore`, `ci`, `build`, `test` and `style` commits are omitted,
unless they introduce a breaking change. Non-conventional commits are omitted.

To refresh the changelog, including the Unreleased section:

```sh
git-cliff --output CHANGELOG.md
```

The entire file is regenerated, so make formatting changes in `cliff.toml`.
Release sections are determined by tags such as `0.1.0`; without release tags,
all entries appear under Unreleased.

To prepare a release:

1. Commit the code changes, update the library version in `Cargo.toml`, and refresh
   `Cargo.lock` with `cargo check -p egui_widgets`.
2. Run `git-cliff --tag 0.1.0 --output CHANGELOG.md`, replacing `0.1.0` with the
   version being released. This labels the pending release without creating a tag.
3. Review and commit `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` with a message
   such as `chore(release): prepare 0.1.0`.
4. Tag that commit with `git tag -a 0.1.0 -m "Release 0.1.0"`, then push the
   commit and tag. Version bumps and crate publishing are handled manually.

For a preview on GitHub, run **Actions → Changelog preview → Run workflow** and
download the `changelog-preview` artifact. The workflow generates a changelog for
the selected branch or tag without committing changes or publishing a release.
