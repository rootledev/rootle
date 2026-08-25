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
- **Find** (`␣ f`) and **grep** (`␣ g`) full-screen, Zed-style: result
  blocks with match chips, folded regions, and per-file counts.
- **Open** any file read-only in your editor (`Enter`); **yank** the
  browser URL of anything (`␣ y`).
- **Clone** repos through a wizard (`v` marks, `:clone`) — orgs fan
  out, destinations get `<dest>/<org>/<repo>`.
- **Configure** in-app with `:settings` (writes config.toml, hot
  reloads themes); every keybinding is in the `?` popup.

## Quick start

```bash
curl -fsSL https://rootle.dev/install.sh | sh   # linux x86_64, to ~/.local/bin
brew install rootledev/tap/rootle   # macOS / linux (builds from source)
cargo install rootle                # or from crates.io (Rust 1.88+)

rootle                # repo search on first run; browser after that
rootle owner/repo     # jump straight into a repo
```

Auth is zero-friction: if `gh auth login` or `ROOTLE_TOKEN` is already
set up, rootle just uses it; anonymous works everywhere except code
search (it says so in the status line when it matters).

## Documentation

| Doc | Contents |
|---|---|
| [settings](https://rootle.dev/docs/settings.html) | every config key, env var, CLI flag |
| [themes](https://rootle.dev/docs/themes.html) | writing your own palette (role reference) |
| [provider protocol](doc/provider-protocol.md) | wrap your own backend (NDJSON-RPC over stdio) |
| [development](doc/development.md) | architecture, dev workflow, e2e harness |
| [house style](doc/house-style.md) | the component behavior contract |
| [provider scaffolding](skills/rootle-provider/SKILL.md) | public skill: scaffold a provider + conformance gate |

## Providers

rootle talks to backends through a small protocol, not to GitHub directly.
`[provider] kind = "github"` is the default in-tree implementation;
`kind = "stdio"` spawns your adapter as a child process — four methods
make a minimal useful provider, and the conformance suite in
[skills/rootle-provider](skills/rootle-provider/SKILL.md) gates correctness.
More providers are planned and the protocol will evolve with them.

## Development

```
cargo test                          # unit + TestBackend render tests
cd e2e && uv run pytest             # PTY end-to-end suite
docker compose run --build --rm test  # fmt + clippy -D warnings + cargo test
docker compose run --build --rm e2e # same e2e suite in a container
```

CI runs the gate + e2e on every push; the `demo` workflow re-renders
the demo GIFs above (one per palette) whenever the app or its tooling
changes and opens a PR with the refreshed artifacts — this README
always shows the current look and feel.
