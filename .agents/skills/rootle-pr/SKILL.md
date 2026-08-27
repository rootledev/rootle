---
name: rootle-pr
description: Author a pull request for rootle — the what/why/how template, the evidence contract (text frames, screenshots, recordings), and the instrumentation steps the agent runs to capture proof of the change before opening the PR. Use when preparing a PR for any non-trivial change.
---

# rootle PR authoring

A rootle PR is a small document with a claims section and an evidence
section. If there is no evidence, there is no PR.

## 1. Before writing anything

- Confirm the definition of done from the relevant plan
  (`plans/NNNN-*.md`) — flip its milestone status in the same PR.
- Green matrix, no exceptions:
  `cargo fmt --check && cargo clippy --all-targets && cargo test`,
  then `cd e2e && uv run pytest`, then
  `docker compose run --build --rm e2e` (the docker gate runs a NEWER
  clippy than the host — it catches lints the host misses; do not skip
  it and be surprised in CI).
- One PR per coherent change; scope creep gets its own PR. Small
  commits inside the PR, theme-grouped, following the repo's
  `feat:/fix:/test:/docs:` style.

## 2. The template

```markdown
## What
<one paragraph, user-visible terms — what behaves differently>

## Why
<the problem or plan; cite plans/NNNN section, issue, or the UX nit>

## How
<bulleted walkthrough: key files, the approach, alternatives rejected
 and why — one line each>

## Verification
- cargo test: <N unit + M render, green>
- e2e: host <K passed>, docker <K passed>
- live PTY run of the changed flow: <what was driven, what was seen>

### Evidence
<text frames and/or screenshots — see §3. Nothing to show for pure
refactors is fine; say so explicitly instead of omitting silently>
```

## 3. Evidence — instrument yourself before opening the PR

Pick by change type; when in doubt, do more:

| Change | Evidence |
|---|---|
| Single-screen visual change | **before/after text frames** + one screenshot |
| Interaction/flow change | **before/after text frames** + short recording |
| Bug fix | **reproduction frame (broken) → frame (fixed)** |
| Non-UI (cache, provider, perf) | command output / trace excerpt |

### Text frames (always — they render in any PR body)

Drive the real binary through the changed flow and dump the screen:

```bash
cd e2e && uv run python - <<'EOF'
import tempfile
from pathlib import Path
from conftest import FS_PROVIDER, make_fs_root, open_fs_repo
from tui import Tui, build

tmp = Path(tempfile.mkdtemp()); root = make_fs_root(tmp)
config = tmp / "p.toml"
config.write_text(
    f'[provider]\nkind = "stdio"\n'
    f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n')
t = Tui(build(), cols=100, rows=28, args=["--config", str(config)]).start()
open_fs_repo(t)          # ...drive to the changed state with expect()
print(t.expect("main.rs"))  # the changed state
t.stop()
EOF
```

Paste the output in a fenced block. Frames are diff-able in review —
reviewers can see exactly which cells changed.

### Screenshots (visual changes)

There is no stills pipeline anymore — frames are the evidence. If a
change is doc-worthy visually, it belongs in the demo tape: extend
`demos/demo.tape` and let the `demo` workflow re-render (see the
[rootle-demo-capture] skill for tape gotchas).

### Recordings (flow changes)

Short VHS tape for the PR thread — do NOT commit unless it replaces
`doc/demo.gif`. Keep it in `/tmp` and reference locally, or regenerate
on request:

```bash
bash e2e/demo_setup.sh
docker run --rm -v "$PWD:/vhs" -w /vhs ghcr.io/charmbracelet/vhs /tmp/<tape>
```

Tape rules ( pacing gotchas in [rootle-demo-capture]): 10ms typing for
shell/launch lines, 60ms in-app, generous sleeps after provider
round-trips.

[rootle-demo-capture]: ../rootle-demo-capture/SKILL.md
[rootle-tui-debug]: ../rootle-tui-debug/SKILL.md
[rootle-provider]: ../../../skills/rootle-provider/SKILL.md

## 4. Opening the PR

```bash
git checkout -b <feat|fix>/<slug>
# ... commits (theme-grouped), push
gh pr create --title "<type>: <summary>" --body-file /tmp/pr-body.md
```

Title mirrors the dominant commit's type. Body = the template, filled
in honestly — every checkbox pre-verified, not aspirational.

## 5. Review checklist (self-review before requesting review)

- [ ] Template complete; evidence shows the CHANGE, not just the app
- [ ] Plans status flipped; docs/screenshots updated in the same PR
- [ ] No stray debug output, no `.cast`/frame artifacts committed
- [ ] Keybindings changed? keybinds popup + hints derive automatically
      — show a frame of the `?` popup if the keymap table changed
- [ ] Provider protocol touched? forge-conformance suite green against
      the fs reference provider (the `forge-conformance` CI job runs it)
