# ghx

ghx is a modal terminal UI (ratatui) for browsing remote
source-control systems: a yazi-style miller-column browser (org, repo,
directory, file) with a syntax-highlighted preview pane. No clone is
required — files open read-only in your editor, browser URLs yank to
the clipboard, and a full-screen search view finds files and greps
content.

**ghx is not tied to any one forge.** Backends plug in behind a
provider seam: GitHub ships in-tree as the reference implementation,
and any other system (GitLab, your internal SCM, …) can be wrapped as
a child process speaking NDJSON-RPC 2.0 over stdio — a small script
conforms it to the protocol. More providers are planned, and the
protocol will evolve with them. Spec:
[doc/provider-protocol.md](doc/provider-protocol.md); scaffolding:
[skills/ghx-provider](skills/ghx-provider/SKILL.md).

![ghx demo](doc/demo.gif)

## Flows

### Browse

On a fresh install ghx opens on the repo search popup; `␣ s` reopens
it at any time, prefilled with the last repo (Enter resumes it).
`ghx owner/repo` skips the popup entirely.

```
type query, Enter   search (orgs + repos)
j / k               move               h / l   focus out / drill in
J / K               scroll the preview
/                   filter the focused pane (Enter commits, Esc cancels)
Enter               open: file → editor (read-only), dir/repo/org → drill in
?                   keybinds popup     :   command line     q  quit
```

Scrolling surfaces carry a scrollbar on the left border — the track is
the border, the thumb the accent.

### Find and grep (global search view)

`␣ f` (find file by path) and `␣ g` (grep contents) open a
full-screen search that replaces the browser until closed.

```
Tab / BackTab       cycle query → scope → extension → results
Enter               in a text field: run the search
j / k on scope      cycle repo / org / global
Enter on scope      radio popup: j/k/g/G move and apply live,
                    Enter commits, Esc reverts
j / k on results    move between hits        J / K   free scroll
/                   filter results by path or preview text
Enter on results    open the hit in the editor (read-only)
Esc                 close the view; the browser is untouched underneath
```

Repo-scope file find runs over the cached tree — no provider call at
all. Grep and global file find use the provider's code search (GitHub
code search requires a token; other providers decide for themselves).

### Clone

`v` enters VISUAL mode; `␣` marks entries — repos, files (fold up to
their repo), or whole orgs (expand down to every repo). Marks persist
after leaving VISUAL; `␣ c` clears them, `␣ d` deletes marked orgs.
`:clone` opens a three-screen wizard: repos, destination, summary.

```
␣                   toggle a mark (●/○)
Tab                 list ↔ buttons            j / k   move / scroll
l / Enter on dirs   descend (destination)     h       go up
Enter on [clone!]   git clone into <dest>/<org>/<repo>
Esc                 cancel the whole wizard from any screen
```

With no marks, `:clone` offers every repo of the selected org. Clones
run on a worker thread; the modeline reports the outcome.

## Install

Each tagged release publishes a static Linux x86_64 musl binary and
its sha256 ([releases](https://github.com/tknawara/ghx/releases));
download, `chmod +x`, run. Build from source with Docker (exports
`./dist/ghx-linux-x86_64-musl`):

```
docker compose run --build --rm release
```

## Auth (GitHub provider)

If your machine is already set up for GitHub — `gh auth login` done,
or `GHX_TOKEN`/`GITHUB_TOKEN` exported — you're done; ghx picks it up.
Anonymous use can browse and preview; only code search needs a token
(the app says so when it does).

Other providers authenticate inside their own adapter — ghx never sees
their credentials.

## Configuration

`~/.config/ghx/config.toml` — missing or malformed falls back to
defaults; editable in the app via `:settings`, which writes the file
back and hot-reloads the theme. The full reference:
[doc/settings.md](doc/settings.md).

```toml
[editor]
program = "nvim"      # optional; default $VISUAL → $EDITOR → hx/nvim/vim/vi
args = []
read_only = true      # adds -R for the vim family

[theme]
name = "catppuccin-mocha"  # overrides in ~/.config/ghx/themes/<name>.toml

[cache]
max_mb = 512          # blob cache cap; LRU eviction + orphan sweep at startup

[provider]
kind = "github"       # or "stdio"
command = []          # argv, used when kind = "stdio"
```

## Providers

`[provider] kind = "github"` is the default. For anything else, run an
adapter process and point at it:

```toml
[provider]
kind = "stdio"
command = ["python3", "/path/to/adapter.py", "…"]
```

The reference adapter is [`examples/providers/fs_provider.py`](examples/providers/fs_provider.py)
— it serves local directories as repos and doubles as the offline e2e
backend. A minimal useful adapter needs four methods; the protocol is
versioned by handshake.

## Development

```
cargo test                          # unit + TestBackend render tests
cd e2e && uv run pytest             # PTY end-to-end suite
docker compose run --build --rm test  # fmt + clippy -D warnings + cargo test
docker compose run --build --rm e2e # same e2e suite in a container
```

Further docs: [doc/getting-started.md](doc/getting-started.md) (first
run, key map, state/cache/config locations),
[doc/development.md](doc/development.md) (architecture, dev workflow,
e2e harness), [doc/settings.md](doc/settings.md) (every setting) and
[doc/house-style.md](doc/house-style.md) (the component contract).
