# gpui-md3e

[Material Design 3 Expressive](https://m3.material.io/) components for
[GPUI](../gpui-reloaded), with spring-driven motion throughout.

```sh
nix develop -c cargo run -p md3e-gallery   # or plain `cargo run -p md3e-gallery`
```

## Setup

```rust
use gpui_md3e::{Md3eAssets, prelude::*, theme::SchemeConfig};

application().with_assets(Md3eAssets).run(|cx| {
    gpui_md3e::init(SchemeConfig::default(), cx); // fonts, theme, key bindings
    // ...
});

impl Render for MyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Theme::tick(window, cx); // animates theme changes
        Button::new("send", "Send").icon(Icon::Send).on_click(|_, _, _| {})
    }
}
```

Apps with their own assets can serve `Md3eAssets::get(path)` from their
`AssetSource` first.

## What is here

**Foundations**

- `theme`: dynamic color from a seed (material-color-utilities' 2025 spec,
  nine scheme variants, light/dark, contrast) that cross-fades on change; the
  baseline and emphasized type scales with CJK and emoji fallbacks; the M3E
  corner scale; elevation shadows for fractional levels.
- `motion`: the six expressive and standard spring tokens, a retargetable
  `SpringValue`, and `animate` for per-element springs in `RenderOnce`
  components. Animations run on `frame_now`, a clock that advances in whole
  display refresh periods, so motion stays even when frames are rendered at
  irregular moments.
- `interaction`: hover, press and keyboard state with state layers, ripples
  clipped to the container and the focus ring.
- `shapes`: the 35 shapes of the M3E library and cached shape morphing
  (via `polymorpher`, a Rust port of androidx `graphics-shapes`).

**Components**

| Component | Notes |
|---|---|
| `Button` | 5 styles × 5 sizes, round/square, toggle; corners morph on press and toggle |
| `IconButton` | standard/filled/tonal/outlined, narrow/default/wide, toggle |
| `Fab` | 3 sizes, 6 color sets, extended FAB that collapses on a spring |
| `FabMenu` | FAB morphs into a close button; actions stagger in |
| `ButtonGroup` | standard (pressed button widens, neighbors yield) and connected |
| `SplitButton` | inner corners soften on hover; menu button turns round when open |
| `LoadingIndicator` | 7-shape morph loop, contained and determinate variants |
| `LinearProgress`, `CircularProgress` | flat or wavy, determinate or indeterminate |
| `Switch`, `Checkbox`, `RadioButton` | handle growth, drawn check mark, springing dot |
| `Slider` | M3E handle with gap, value bubble, steps and stop indicators |
| `Chip` | assist, filter (animated check), input, suggestion |
| `TextField` | filled/outlined, floating label, IME composition (the candidate window follows the caret), submit, several lines (`multiline(n)`: wraps, Shift+Enter for a new line, Up/Down between lines, grows to n lines then scrolls), `TextFieldEvent::Edited` on the user's edits, masked (passwords), scrolls to the caret, double/triple click selects a word/all, drag the selection to move it, accepts dropped text, also from other applications |
| `SelectableText` | read-only text the reader selects with the mouse (drag, double and triple click), copies (Ctrl+C) and drags into a text field or, on Wayland, into other applications |
| `NavigationRail` | collapsed ↔ expanded with continuous item reflow |
| `Card`, `ListItem`, `Toolbar`, `Snackbar` | including segmented (grouped) list items |
| `ShapeView` | any library shape, morphing on hover |

## Gallery and scripted testing

`md3e-gallery` shows every component. It can also drive itself, which is how
the components are checked without a person at the keyboard:

```sh
MD3E_PAGE=3 MD3E_SCRIPT="move 474 335; down; wait 90; shot /tmp/press.png; up; quit" \
  cargo run -p md3e-gallery
```

Commands: `move X Y`, `down`, `up`, `click X Y [count]`, `key <keystroke>`,
`type <text>`, `scroll <lines>`, `wait <ms>`, `shot <png>` (runs `grim`),
`quit`. Events are dispatched into the gallery window only. The driver is
`gpui_md3e::script`, so apps built on gpui-md3e can script themselves the
same way.

## Known gaps

- No letter spacing (GPUI has no tracking yet), so type tokens ignore it.
- GPUI has no element transforms, so scale effects are done with layout.
- Ripples clip to convex containers only.
- Not yet: menus, dialogs, tooltips, tabs, app bars, search, date pickers.

## Licenses

- Code: Apache-2.0. Token values follow Jetpack Compose Material 3
  (Apache-2.0).
- Roboto (`assets/fonts/roboto`): SIL Open Font License 1.1. The static
  weights are instances of the variable font from Google Fonts.
- Material Symbols (`assets/icons`): Apache-2.0.
