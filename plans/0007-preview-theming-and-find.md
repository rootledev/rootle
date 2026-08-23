# 0007 — Theme-driven syntax highlighting + find-in-file (v0.3.2)

Status: implemented (v0.3.2).

## 0. Context

Two preview-pane gaps, one milestone:

1. **Syntax colors ignore the selected theme.** `Highlighter::new()`
   hardcodes the Catppuccin Mocha syntect theme and is built once in
   `App::new`. Switching themes (settings popup, `--theme`, config)
   restyles the chrome but not the code. `Semantic` carries no syntax
   roles, so there is nothing to map from today.
2. **No search inside the previewed file.** `/` filters the focused
   miller list (house style: every list is filterable — unchanged).
   Reading a file wants the vim model instead: highlight all matches,
   `n`/`N` to jump. The 0006 line cursor already anchors `␣ y` yank
   URLs (`repo/web_url` with `line`), so find-in-file needs no yank
   changes: moving the cursor to a match makes the URL line-anchored
   for free.

Key-binding decision (locked): **`␣ /` = find-in-file.** `/` keeps its
single meaning (list filter) because the preview never owns the
keyboard — a context-split `/` would be ambiguous while browsing a
file list with a preview open. A focusable preview pane (yazi-style)
was rejected for this milestone: new modal concept, every key
re-audited, no demonstrated need beyond this feature.

## 1. Syntax roles on `Theme`

`src/theme.rs`:

- New `pub struct Syntax { keyword, string, comment, function,
  type_, constant, tag, namespace, invalid: Color }`; `Theme` gains
  `pub syntax: Syntax`. Mocha defaults are the compile-time baseline,
  same pattern as `Semantic`.
- Every embedded palette (Dracula, One Dark, Gruvbox Dark, Nord,
  Tokyo Night, Solarized Dark, Catppuccin Latte, GitHub Light,
  One Light, Solarized Light) lists its syntax roles explicitly from
  its published spec — parallel `SYNTAX` const tables next to the
  existing `RoleValue` ones.
- Palette TOML overrides gain a `[syntax]` section mirroring
  `[semantic]`; `ThemeOverrides` + a `set_syntax_role` shared by
  files and embedded tables. Unknown role names ignored, as today.

`src/highlight.rs`:

- `Highlighter::new(theme: &Theme)` builds the `STheme` from
  `theme.syntax` (+ `text` for variables/plain, `base` unused —
  span backgrounds stay `None` so the terminal/pane bg shows through).
  The existing scope-selector table is unchanged; only the hexes move.
- `Default` uses `Theme::catppuccin_mocha()` so existing tests keep
  working.

## 2. Re-highlight on theme change

Root cause of the stale colors, beyond the hardcoded palette: blobs
are highlighted once (`Action::BlobLoaded`) and cached as styled
`Line`s (`Browser::blobs: HashMap<sha, Vec<Line>>`). Restyling needs
the raw text.

- `Browser::blobs` becomes `HashMap<String, String>` — sha →
  *sanitized raw text*. `blob_loaded` stores text; the refresh path
  highlights on selection via the app's highlighter.
- `App::highlighter` rebuilds (`Highlighter::new(&new_theme)`) when
  the effective theme changes: settings-popup commit today, plus the
  popup's live preview row (theme under the cursor applies live,
  matching the "radio follows the cursor" house rule).
- After a rebuild, the browser re-highlights the currently previewed
  file from cached raw text. Other files re-highlight on next
  selection — no refetch, the raw cache survives.
- Global search hit previews (`finish_hits`) already go through
  `self.highlighter`, so they follow with no extra work.

Cost: one extra `String` per cached blob vs. today's `Vec<Line>` —
roughly neutral; highlight CPU per selection is unchanged (it
happened once per blob before, now once per selection).

## 3. Find-in-file (`␣ /`)

New mode + state; the yank path is untouched.

**Mode.** `Mode::Find`, chip `FIND` (uses `mode_search` color).
Dispatch: keys route to a dedicated transient `VimInput` on the
browser (same shape as the list filter input — live update on every
keystroke). Hints row: `type query · enter jump · esc cancel`,
derived from `keymap::hints(Mode::Find)`.

**Keys.**

- `␣ /` (leader table) → `Action::LeaderFindInFile`: enters FIND if
  the preview holds text content (plain or highlighted); dir
  summaries and binaries get a `not a text file` status toast.
- Typing: incremental match computation + chips; cursor follows the
  first match as the query grows.
- Enter → commit: back to BROWSE, matches stay chipped, cursor on
  the nearest/first match.
- Esc → cancel: back to BROWSE, chips cleared, cursor restored to
  its pre-find line (vim restores position on cancelled `/`).
- `n` / `N` in BROWSE (both unbound today) → `Action::FindNext` /
  `FindPrev`: cycle per occurrence with wraparound; the line cursor
  lands on the occurrence's line. No active find → no-op.
- Esc in BROWSE precedence: active find highlights clear first, then
  the committed list filter (extends the existing "first Esc clears"
  contract).

**Matching.** Case-insensitive substring, per occurrence (several on
one line each chip and each is an `n` stop). Computed on the plain
text of the cached blob (char-boundary safe; same lowercased-bytes
approach as `global_search::highlight_matches` — ASCII-exact,
cosmetic drift on exotic folds is accepted there already).

**Rendering.** `Preview` owns `Option<FindState { query, matches,
current }>`; chips are applied at render time by splitting the
(syntax-styled) spans, so `n`/`N` never recompute highlighting.
Match chip = `search_match` bg + `crust` fg (the grep-view chip);
the current match swaps bg to `warning` so it reads at a glance.
The preview border readout becomes `3/12 · 7/41` (match of matches ·
line of total) while a find is active.

**Yank.** `␣ y` already anchors `repo/web_url` to the cursor line —
after `n` lands on a match, yank produces `…#L42` with no new code.

## 4. Preview chrome: line numbers, scrollbar, footer

Research baseline: yazi and bat both ship line numbers + a scroll
position cue; glow adds rendered structure; superfile leans on icons.
What fits rootle's constraints (remote blobs, musl-static, no image
protocols):

**Line numbers (bat/yazi parity).** Left gutter inside the preview
border for text content (plain + highlighted):

- Right-aligned, width = digits(`line_count`), one separating space;
  fg `overlay0` (bat's dimmed gutter). The cursor line's number gets
  `text` + bold (vim's `CursorLineNr` behavior) so `J/K`/`n` movement
  reads at a glance.
- Render-time prefix span per line — wrapped continuation lines carry
  no number (bat behavior, free from `Paragraph::wrap`).
- The existing cursor-line `selection_bg` tint extends over the
  gutter (vim's cursorline does the same).
- No gutter for dir summaries, binaries, empty.

**Scrollbar (house-style fix).** The preview scrolls but never calls
`components::scrollbar` — "any content that scrolls shows one" makes
this a style bug, not a feature. Same call shape as `pane.rs`: track
on the right border, thumb `border_focused`, nothing when content
fits. `n`/`N` jumps already re-clamp scroll via `clamp_scroll`.

**Footer metadata.** `title_bottom` left-aligned: syntax name + line
count (` rust · 41 lines `); the `m/n · l/total` readout stays
right-aligned. The syntax name comes from the highlighter —
`highlight` already resolves the `SyntaxReference`; return its name
alongside the lines and store it on `Preview` via `set_highlighted`.
Plain-text fallback shows ` text · N lines `.

**Tab expansion buglet.** The `Text` path replaces `\t` with four
spaces; the `Highlighted` path passes syntect spans through raw, so
tabs jump to terminal stops mid-line. Expand tabs per span at the
highlight boundary (styling preserved), matching the plain path.

## 5. Whole-TUI eye candy (research outcome)

**Adopted in this milestone:**

- **Dim unfocused miller columns** (yazi/superfile pattern). Focus
  currently reads only from the border color. Unfocused panes render
  entries in `subtext0` (dirs keep `directory` but dimmed via
  `Modifier::DIM`, no bold); the selection row and marks stay
  visible, muted. One render branch in `pane.rs` on the existing
  `focused` flag; no new theme roles.

**Deferred (own plan, noted here so they aren't re-litigated):**

- **Nerd Font file/dir icons** — opt-in `[ui] icons = true`; font
  dependency makes it a divisive default, and the entry model needs
  an icon column first.
- **Rendered markdown preview** (glow-style) for `*.md` — a real
  renderer, not a tweak.
- **Relative line numbers** — trivial add once the gutter exists,
  but only behind config if someone asks.

**Rejected:**

- **Image previews** (kitty/sixel/iTerm2): remote blobs + per-terminal
  protocol matrix + static musl — yazi dedicates a whole subsystem to
  this; no proportionate win for a source browser.
- **Git blame / change gutter**: the provider protocol has no per-line
  history method and the REST cost is per-file-per-line; revisit only
  if a provider grows the method.

## 6. Version

`Cargo.toml` 0.3.1 → **0.3.2** in the same PR (after the feature
lands, so the tag built from a merged main contains it). Push
`v0.3.2` to trigger the release workflow. 0004-v0.4 stays the next
minor line; this is a patch release off current main.

## 7. Milestones

One PR, four commits:

1. **Theme-driven syntax** — `Syntax` roles, all palettes, TOML
   `[syntax]`, `Highlighter::new(&Theme)`, raw-text blob cache,
   rebuild + re-highlight on theme change (incl. settings live
   preview).
2. **Preview chrome** — line-number gutter, scrollbar, footer
   metadata, tab expansion fix, dimmed unfocused panes.
3. **Find-in-file** — `Mode::Find`, leader binding, find input,
   match state + chips, `n`/`N`, Esc precedence, readout.
4. **Release** — version bump; plans/0007 status flip.

## 8. Verification

- Unit: every embedded palette loads complete syntax roles (mirror
  the existing semantic-role test); `[syntax]` TOML override merges
  and leaves siblings at defaults; match computation (case-fold,
  multiple per line, empty query, no match); `n`/`N` wraparound;
  cancel restores the cursor line; gutter width from line count;
  tab expansion preserves span styling.
- Render (TestBackend): syntax chip colors differ between two themes
  for the same file; number gutter renders (dim, current line
  emphasized); scrollbar thumb appears on overflowing content and
  tracks `J/K`; footer shows ` rust · N lines `; unfocused panes
  render dimmed; find chips render on styled spans; current-match
  chip distinct; readout shows `m/n · l/total`; no residue after Esc.
- e2e (fs provider): switch theme in settings → preview code colors
  change on screen (pyte cell fg); long file → scrollbar thumb;
  `␣ /` type query Enter → cursor readout on first match; `n`
  advances, wraps; `␣ y` after `n` yanks a `#L`-anchored URL
  (`ROOTLE_CLIPBOARD`); Esc clears chips, second Esc clears the list
  filter.
- Docker gate + full matrix per the PR skill; demo artifacts PR
  follows via the `demo` workflow.
