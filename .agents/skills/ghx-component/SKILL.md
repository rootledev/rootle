---
name: ghx-component
description: Scaffold a new ghx UI component following the project's Component trait contract, keymap-table registration, theming rules, and test requirements. Use when adding any new screen, popup, pane, or widget-level component to ghx.
---

# Scaffolding a ghx component

All components live in `src/components/` and implement the shared
contract from `src/components/mod.rs`:

```rust
pub trait Component {
    fn handle_key(&mut self, key: KeyEvent) -> Action;
    fn update(&mut self, action: &Action);
    // &mut self: ratatui's stateful widgets (ListState) require it
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme);
}
```

## Checklist for a new component `foo.rs`

1. **State struct** — plain fields, no global access. Components never
   reach into `App`; data arrives via `update(&Action)` or constructor.
2. **`handle_key`** — map keys to `Action`s and return them. Never
   mutate state here beyond local, non-dispatched concerns (e.g. cursor
   blink); everything else flows back through `update`.
3. **Keymap table** — register the component's keys in
   `src/keymap.rs` under its mode(s). The modeline hints are *derived*
   from this table: if a key isn't in the table, it must not work, and
   vice versa. No out-of-band key handling.
4. **Theming** — colors come ONLY from `Theme` semantic roles
   (`theme.semantic.*`), never a hardcoded `Color::`. If a needed role
   doesn't exist, add it to the palette schema (`src/theme.rs`) with a
   Catppuccin Mocha default.
5. **Render** — draw into the caller's `Rect`; never assume full screen.
   Borders rounded, title in top border per the design language
   (PLAN.md §5).
6. **Text safety** — any string originating from the network or files
   goes through `sanitize.rs` before being stored in render state.
   Width math via `unicode-width`.
7. **Test** — a `TestBackend` snapshot test rendering the component in
   its important states (empty, populated, focused/unfocused, filtered).

## Composition rules

- A parent (e.g. `Browser`, `SearchPopup`) owns children, tracks focus,
  and forwards `handle_key` only to the focused child.
- Children never know their parent; cross-component effects go through
  `Action` and the root dispatcher in `app.rs`.
- Reuse the library: `VimInput`, `ListView`, `Popup`, `Pane`,
  `Modeline`, `Preview`. If you're copying one of these, stop — extract
  a parameter instead.

## Popup specifics

- Wrap content in the `Popup` shell (handles centered rect, `Clear`,
  rounded border, title, hint row).
- `<Esc>` dismisses. If the popup owns a `VimInput` in INSERT mode, the
  first `<Esc>` goes to the input (→ NORMAL), the second dismisses.
- Closing a popup must set the flag that forces a full redraw.

## Anti-patterns (rejected in review)

- `Color::Red`-style hardcoded colors anywhere in `src/components/`.
- `match` on keys inside `app.rs` — dispatch belongs to the keymap table.
- Components calling each other directly or sharing `&mut` state.
- Rendering with `format!`-padded strings instead of layout-aware
  truncation.
