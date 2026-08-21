# Getting started

Two minutes from install to browsing remote repos — no clone, no
config file. This page is the ramp: install, launch, the handful of
keys that do everything. Every knob lives in
[settings.md](settings.md); wrapping your own backend in
[provider-protocol.md](provider-protocol.md).

## Install

Grab the static musl binary from
[github.com/tknawara/rootle/releases](https://github.com/tknawara/rootle/releases)
(each release ships `rootle-linux-x86_64-musl` plus a `.sha256`), or:

```
cargo install rootle                      # from crates.io (Rust 1.88+)
docker compose run --build --rm release   # → ./dist/rootle-linux-x86_64-musl
```

## First launch

```
rootle                # search popup on a fresh install
rootle owner/repo     # skip the popup, open a repo directly
```

A fresh install opens on the search popup. Type a query, press Enter;
results list orgs first, then repos — `j/k` move, Enter opens. Every
later launch skips this and opens the browser straight away with your
recent orgs/repos (`␣ s` reopens the popup, prefilled with the last
repo — Enter alone resumes it).

![launch search popup](img/01-launch-search.png)

## The browser

Three miller columns — orgs → repos → tree — plus a live
syntax-highlighted preview of whatever the cursor is on. `j/k` move,
`h/l` step out/in a level, `J/K` walk a line cursor through the
preview (the border shows `line/total`), `/` filters the focused pane.

![browsing with the preview pane](img/03-browse.png)

## The keys that matter

| Key | Does |
|---|---|
| `j` `k` `h` `l` | move · focus out · drill in |
| `Enter` | open: repo, tree, or file |
| `␣` | leader — `␣ s` search, `␣ f` find file, `␣ g` grep, `␣ y` yank URL |
| `v` | VISUAL multi-select (marks for `:clone`) |
| `:` | command line — `:settings`, `:clone` |
| `?` | every keybinding, per mode |
| `q` | quit |

The `?` popup is generated from the same tables that drive key
dispatch, so it can never drift from reality. In any popup or text
input: `Tab` moves between fields, `Enter` submits, `Esc` steps back —
inputs are modal like vim (`Esc` drops to NORMAL, `i/a/A` re-enter
INSERT).

![keybinds popup](img/08-keybinds.png)

## Finding things

Three searches, one shape (`␣ f` / `␣ g` replace the browser
full-screen):

- **`␣ s` — repos & orgs.** The launch popup, reopened.
- **`␣ f` — find file.** Matches whole paths like GitHub's *go to
  file*: needles match contiguously or as an in-order subsequence
  (`urldef` finds `djangosite/urls/default.py`), filename hits rank
  above directory hits. Space separates needles; all must match.
- **`␣ g` — grep contents.** Zed-style result blocks: path +
  match-count badge, syntax-highlighted lines with real numbers,
  matched text chipped in yellow. `/` filters results locally,
  `Enter` on a hit opens it in your editor.

Scope waterfalls from where you are — an open repo defaults to
`repo:`, a selected org to `org:`, otherwise `global`. `j/k` on the
scope field cycles it; `Enter` opens the radio popup with live
preview.

![grep results](img/04-grep.png)

![scope popup](img/06-scope-popup.png)

## Open & yank

`Enter` on a file materializes the cached blob under
`~/.cache/rootle/edit/`, suspends the terminal, and runs your editor
on it read-only — edits are never written back. Resolution:
`[editor].program` → `$VISUAL` → `$EDITOR` → first of `hx`, `nvim`,
`vim`, `vi`.

`␣ y` copies the browser URL of whatever is under the cursor — repo,
org, file, tree, or a search hit with a `#L<line>` fragment. In the
browser's preview the line cursor sets the fragment: move with `J/K`,
yank the URL of that exact line. Clipboard: OSC 52 (works over SSH and
tmux), then a local tool.
`ROOTLE_CLIPBOARD=<path>` redirects yanks to a file for scripts and
CI.

![yank toast](img/07-yank.png)

## Clone repos

`v` enters VISUAL; `Space` marks entries (the marks persist after you
leave VISUAL). `:clone` resolves marks to repos — a marked org expands
to all of them — then walks three screens: repo checkboxes, a
destination mini-browser over your local folders, and a summary with
the exact commands. Repos land at `<dest>/<org>/<repo>` so same-named
repos never collide. `␣ c` clears marks; `␣ d` deletes marked orgs
from the orgs pane and your recents.

![clone wizard: repos](img/12-clone-repos.png)

![clone wizard: summary](img/14-clone-summary.png)

## Auth

If your machine already talks to GitHub — `gh auth login` done once,
or `ROOTLE_TOKEN`/`GITHUB_TOKEN` exported — rootle just uses it.
Everything else works anonymously; only code search asks for a token,
and it says so in the status line when it does. With a stdio
provider, credentials live entirely inside your adapter — rootle
never sees them.

## Going further

- [settings.md](settings.md) — every config key, theme role, env var,
  CLI flag, and where state/config/cache live on disk.
- [provider-protocol.md](provider-protocol.md) — run rootle against
  your own backend via a stdio provider.
- [development.md](development.md) — architecture, dev workflow, and
  the test harness, for contributors.
- [house-style.md](house-style.md) — the component contract.
