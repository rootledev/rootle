---
name: rootle-demo-capture
description: Record rootle's demo GIFs — one VHS tape, rendered once per embedded palette (canonical demo.gif + demo-<theme>.gif variants, published to the site repo rootledev/rootledev.github.io for the README hero and the palette picker). Carries every gotcha hit while building this pipeline (frame collapse, font fallback, PTY races). Use when regenerating the demo GIFs.
---

# rootle demo capture

One tape, eleven renders — one per embedded palette:

| Artifact | Palette | Used by |
|---|---|---|
| `demo.gif` | catppuccin-mocha | README hero, site default |
| `demo-<theme>.gif` | the other ten | site palette picker |

Local renders land in gitignored `demos/out/`; the published GIFs live
in the site repo (`rootledev/rootledev.github.io`, `img/`). The `demo`
workflow renders all of them on pushes touching `src/`, `demos/`, or
`e2e/` and opens a `demo/artifacts` PR on the site repo — prefer
letting CI do it. To render locally:

```
docker compose run --build --rm -e VERSION=0.0.0-demo release   # binary for the tape
docker run --rm -v "$PWD:/vhs" -w /vhs --entrypoint sh ghcr.io/charmbracelet/vhs -c \
  "mkdir -p /usr/share/fonts/truetype/jbmono && \
   cp /vhs/demos/fonts/JetBrainsMono-*.ttf /usr/share/fonts/truetype/jbmono/ && \
   fc-cache -f >/dev/null 2>&1; exec vhs demos/demo.tape"
```

Per-theme locally: sed the tape like the workflow does — `Output` path
plus `--theme <name>` on the launch line — then render as above.

Gotchas (all were hit, all cost time):

- **Do not render GIFs from `.cast` recordings with agg.** ratatui
  writes full-screen diffs; agg's frame assembler collapses them into
  a few frames. Record the real terminal with VHS instead. (The cast
  recorder, `demos/demo.py`, stays for debugging only.)
- **Fast shell typing, slow app typing**: `Set TypingSpeed 10ms` for
  the two setup/launch `Type` lines, then bump to `60ms` before any
  app interaction — the demo should show human typing.
- **Provider round-trips take real time** (~0.3–1s each over stdio).
  `Sleep` generously after every state change (≥800ms after search
  submits, ≥1.2s after popups open) or the recording races ahead of
  the app.
- VHS `Set Theme` paints the shell lines and the window remainder
  (rows×cell-height < Height) — the render matrix maps each palette to
  its VHS theme (`vhs themes` lists valid names), or light renders get
  a dark band. The app's own colors come from `--theme`, not VHS.
- `demos/demo_setup.sh` must run INSIDE the tape (it writes the config
  with `$PWD` of the mounted repo). It extracts the release binary
  from `dist/*.tar.gz`, falling back to a debug build.
- The GIF loop: end the tape with the app quit (`q`) and a small
  `Sleep`, so the last frame isn't mid-redraw.
- VHS resolves fonts via fontconfig: the vendored JetBrains Mono must
  be copied into the container and `fc-cache`'d before `vhs` runs.

## When to re-capture

- UI/visual changes to any flow shown (search, columns, grep, yank,
  wizard, keybinds popup).
- Theme/palette changes: every themed GIF re-renders — keep
  `src/theme.rs` and the workflow's theme list in sync when adding a
  palette (also: the site repo's palette blocks + picker buttons).
- Tape changes: verify the sed transform still matches after editing
  the `Output` or launch lines.

Land the artifacts via the `demo/artifacts` PR on the site repo —
never hand-edit GIFs.
