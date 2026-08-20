---
name: ghx-demo-capture
description: Record the demo GIF and the per-feature screenshots for ghx docs — VHS tape for video, the pyte renderer for stills. Carries every gotcha hit while building this pipeline (frame collapse, color mapping, font fallback, PTY races). Use when regenerating doc/demo.gif or doc/img/*.
---

# ghx demo capture

Two artifacts, two different tools — do NOT cross them:

| Artifact | Tool | Source |
|---|---|---|
| `doc/demo.gif` (README hero) | VHS via docker | `doc/demo.tape` |
| `doc/img/*.png` (getting-started stills) | pyte screen renderer | `e2e/shots.py` |

## 1. Demo GIF

```
bash e2e/demo_setup.sh   # fixture: /tmp/ghx-demo (idempotent)
docker run --rm -v "$PWD:/vhs" -w /vhs ghcr.io/charmbracelet/vhs doc/demo.tape
```

Gotchas (all were hit, all cost time):

- **Do not render GIFs from `.cast` recordings with agg.** ratatui
  writes full-screen diffs; agg's frame assembler collapses them into
  a few frames. Record the real terminal with VHS instead. (The cast
  recorder stays in the harness for debugging only.)
- **Fast shell typing, slow app typing**: `Set TypingSpeed 10ms` for
  the two setup/launch `Type` lines, then bump to `60ms` before any
  app interaction — the demo should show human typing.
- **Provider round-trips take real time** (~0.3–1s each over stdio).
  `Sleep` generously after every state change (≥800ms after search
  submits, ≥1.2s after popups open) or the recording races ahead of
  the app.
- VHS theme: `"Catppuccin Mocha"` (matches the app); agg lacks a
  Catppuccin theme entirely — another reason it's the wrong tool.
- `e2e/demo_setup.sh` must run INSIDE the tape (it writes the config
  with `$PWD` of the mounted repo).
- The GIF loop: end the tape with the app quit (`q`) and a small
  `Sleep`, so the last frame isn't mid-redraw.

## 2. Screenshots

```
cd e2e && uv run python shots.py     # writes ../doc/img/*.png
```

`shots.py` drives the real binary over the fs stdio provider and
renders the pyte screen to PNG **cell-by-cell** (fg/bg per cell,
DejaVu Sans Mono + Sans fallback). Frames are taken right after
`expect()` — deterministic, no timing races. Prefer editing
`shots.py` (the flow, the fixture) over post-editing PNGs.

Renderer gotchas (each produced a visible bug class):

- **Fresh canvas per frame.** The Renderer must recreate the image in
  `render()`; skipping space cells on a reused canvas superimposes
  every previous frame (ghost popups).
- **All-digit hex colors.** `"313244"` (surface0) is `.isdigit()` →
  the xterm-256 branch overflows and PIL clamps to **white popups**.
  Palette indexes are ≤ 255; any 6-char value is RGB hex. Order the
  checks accordingly.
- **Font coverage.** DejaVu Sans **Mono** lacks `▌ ⋮ ● ○ ␣ ▸` (tofu
  boxes). Fallback to DejaVu Sans for codepoints missing from the
  mono faces, detected via fontTools cmaps at startup.
- **Cell metrics.** `cw/ch` from `font.getmetrics()`; height minus
  1px so box-drawing borders connect vertically; width from the max
  mono/bold `getlength("M")` so glyphs never bleed into neighbors.
- Known cosmetic artifact: descenders spill 1px past the bottom row.
- The screen must be pumped (pyte fed) before rendering — `shots.py`
  only shoots after `expect()`, which pumps.

## 3. When to re-capture

- UI/visual changes to any feature shown (fields row, chips, popups,
  wizard screens, keybinds layout).
- New flows: extend `shots.py` with the flow + `shot("NN-name.png")`,
  then embed in `doc/getting-started.md` next to the feature text.
- Theme/palette changes: re-run both (GIF and stills).

Commit the artifacts — GitHub renders both inline in README/docs.
