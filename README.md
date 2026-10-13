# egui_widgets

Reusable widgets for [egui](https://github.com/emilk/egui).

## Widgets

- `WheelBox` – numeric input box for integers (`i8`–`i64`, `u8`–`u64`) and floats (`f32`, `f64`).
  Click to focus, then:
  - `↑` / `↓` or the mouse wheel steps the value,
  - `Q` / `W` halves / doubles the step, `R` resets it,
  - typing a number and pressing `Enter` (or moving the focus away) commits it, `Esc` discards it.

  `decimals` only limits what is shown, the value keeps its full precision.

  ```rust
  ui.add(WheelBox::new(&mut count).range([0, 100]).prefix("n: "));
  // floats default to 3 decimals
  ui.add(WheelBox::new(&mut exposure).decimals(2).suffix(RichText::new("ms").weak()));
  ```

  `prefix` / `suffix` take egui [atoms](https://docs.rs/egui/latest/egui/struct.Atom.html)
  (text, `RichText`, images, or tuples of them) and stay visible while editing.
  If the box is too narrow, the suffix is hidden first, then the prefix, and only then is the
  value truncated.

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

- Unit tests for the pure value logic (parsing, stepping, clamping) are next to the code in `src/`.
- Interaction tests in `tests/` drive the widgets headlessly with
  [`egui_kittest`](https://crates.io/crates/egui_kittest). The widgets are exposed to AccessKit
  as text inputs whose value is the displayed number, so tests can find them by value.
