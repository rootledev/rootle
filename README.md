<div align="center">

<img src="https://rootle.dev/assets/logo.svg" alt="rootle" width="480">

**rootle — browse remote source-control systems from your terminal**

**status: alpha** — features, keys, and config may still shift between
releases; pin a version if it matters to you.

[![ci](https://github.com/rootledev/rootle/actions/workflows/ci.yml/badge.svg)](https://github.com/rootledev/rootle/actions/workflows/ci.yml)
[![release](https://github.com/rootledev/rootle/actions/workflows/release.yml/badge.svg)](https://github.com/rootledev/rootle/actions/workflows/release.yml)
[![audit](https://github.com/rootledev/rootle/actions/workflows/audit.yml/badge.svg)](https://github.com/rootledev/rootle/actions/workflows/audit.yml)
[![crates.io](https://img.shields.io/crates/v/rootle.svg)](https://crates.io/crates/rootle)
[![version](https://img.shields.io/github/v/release/rootledev/rootle?display_name=tag&sort=semver)](https://github.com/rootledev/rootle/releases/latest)
[![status](https://img.shields.io/badge/status-alpha-orange)](https://github.com/rootledev/rootle/milestones)
[![website](https://img.shields.io/badge/website-rootle.dev-89b4fa)](https://rootle.dev/)

A modal TUI (ratatui) with a yazi-style miller-column browser,
syntax-highlighted previews, Zed-style global search, and a pluggable
provider seam — GitHub ships in-tree, anything else wraps in via a
small stdio script.

![demo](https://rootle.dev/assets/demo.gif)

</div>

## Contents

- [What it does](#what-it-does)
- [Quick start](#quick-start)
- [Documentation](#documentation)
- [Providers](#providers)
- [Development](#development)

## What it does

- **Browse** orgs → repos → trees → files in three miller columns with
  a live syntax-highlighted preview — no clone required.
- **Revise** — `␣ b` switches branches/tags (`rootle owner/repo@ref`
  from the CLI); `␣ p h` file history with open-at-commit; `␣ p b`
  blame run-margins; yanks from history anchor to the commit sha.
- **Search** — find (`␣ f`) and grep (`␣ g`) with a real grammar
  (`"quoted"`, `-negation`, `language:rust`), results streaming into
  decorated per-file boxes with facet chips; `Enter` opens the whole
  file at the match line.
- **The pane** — `␣ p` focuses and zooms the preview with vim's
  vertical motions (`3j`, `gg`/`G`, `^D`/`^U`, `{`/`}`, `%`, `zt`,
  `:42`).
- **Open** any file read-only in your editor (`Enter`); **yank** the
  browser URL of anything (`␣ y`).
- **Clone** repos through a wizard (`v` marks, `:clone`) — orgs fan
  out, archived grey out, sorted by last push.
- **Configure** in-app with `:settings` (writes config.toml, hot
  reloads themes and chrome); every keybinding is in the `?` popup.
- **Update** — the modeline chips `↑ vX.Y.Z` when a release is newer
  (24h-cached, silent offline); `rootle update` self-updates tarball
  installs with checksum verification. `CHANGELOG.md` rides every
  release.

## Quick start

```bash
curl -fsSL https://rootle.dev/install.sh | sh   # linux x86_64, to ~/.local/bin
brew install rootledev/tap/rootle   # linux (builds from source)
brew install --cask rootledev/tap/rootle   # macOS (prebuilt binary)
cargo install rootle                # or from crates.io (Rust 1.88+)

rootle                # repo search on first run; browser after that
rootle owner/repo     # jump straight into a repo
rootle owner/repo@release/2.7   # …at a branch, tag, or sha
rootle update         # self-update (tarball installs), or your channel's hint
```

Auth is zero-friction: if `gh auth login` or `ROOTLE_TOKEN` is already
set up, rootle just uses it; anonymous works everywhere except code
search (it says so in the status line when it matters).

## Documentation

| Doc | Contents |
|---|---|
| [settings](https://rootle.dev/docs/settings.html) | every config key, env var, CLI flag |
| [themes](doc/themes.md) | writing your own palette (role reference) |
| [provider protocol](doc/provider-protocol.md) | wrap your own backend (NDJSON-RPC over stdio) |
| [development](doc/development.md) | architecture, dev workflow, e2e harness |
| [house style](doc/house-style.md) | the component behavior contract |
| [provider scaffolding](skills/rootle-provider/SKILL.md) | public skill: scaffold a provider |

## Providers

rootle talks to backends through a small protocol, not to GitHub directly.
`[provider] kind = "github"` is the default in-tree implementation;
`kind = "stdio"` spawns your adapter as a child process — four methods
make a minimal useful provider, and the canonical
[forge-conformance](https://github.com/rootledev/forge-conformance)
suite gates correctness (rootle's own CI runs it against the fs
reference provider). GitLab and Bitbucket ship as managed one-binary
adapters; the roadmap for what's next lives on
[rootle.dev/docs/roadmap.html](https://rootle.dev/docs/roadmap.html).

The built-in manager installs stdio providers as verified binaries:

```
rootle provider install gitlab                 # bare name → rootledev/rootle-gitlab
rootle provider install owner/repo@v0.1.0      # GitHub releases, tag-pinned
rootle provider install https://artifacts.corp.example/p/rootle-gitlab-0.1.0-x86_64-unknown-linux-musl.tar.gz
rootle provider install myprovider --path /opt/providers/rootle-myprovider
```

Every networked install is checksum-verified against the mandatory
`.sha256` sidecar, whatever the host. Sources split into two
deployment shapes: **releases-API sources** (github.com slugs/URLs)
are tracked — `rootle provider update` refreshes their latest-known
tags and `upgrade` swaps binaries. **Plain-HTTP and `--path`
installs are install-and-pin** — `update`/`upgrade` never touch them;
upgrades come from whatever deployed them (a config manager, an
artifact-publishing pipeline). `--path` is a first-class deployment
shape, not a testing convenience: it is the steady state for
config-managed installs.

## Development

```
cargo test                          # unit + TestBackend render tests
rootle --headless script.txt        # scripted driver: keys in, frames + state JSON out
cd e2e && uv run pytest             # headless + PTY end-to-end suites
docker compose run --build --rm test  # fmt + clippy -D warnings + cargo test
docker compose run --build --rm e2e # same e2e suite in a container
```

`--headless` (plans/0023) drives the real app without a terminal —
`keys`/`settle`/`frame`/`state` script steps in, plain-text cell grids
and state JSON out — the deterministic surface for tests, reviews, and
agent-driven stress runs. See `src/headless.rs`'s module docs for the
script language.

CI runs the gate + e2e on every push; the `demo` workflow re-renders
the demo GIFs above (one per palette) whenever the app or its tooling
changes and opens a PR with the refreshed artifacts — this README
always shows the current look and feel.
