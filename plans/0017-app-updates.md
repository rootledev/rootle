# 0017 — App updates: changelog, update notice, `rootle update`

Status: **done (2026-08-27)** — changelog landed, `rootle update`
self-updates tarball installs (verified, atomic), the modeline chips
`↑ vX.Y.Z` (24h-cached startup check, silent offline, `[update]
check = false` disables).

## Problem

rootle ships four channels (install.sh tarball, brew formula/cask,
cargo, mise) and none of them tell the user a newer release exists.
Tarball installs have no upgrade story at all short of re-running
install.sh. And there is no changelog — the release notes are
`--generate-notes` PR lists.

## Milestones

### M1 — CHANGELOG.md

Curated, keepachangelog-shaped, one section per tag, user-visible
terms. Written in the release PR (the version-bump PR carries its
section — no post-hoc editing). The GitHub release body links to the
section; the site's installbar/version chip links to the changelog
page. Backfill 0.6.0–0.7.1 from the merged PR set when this lands.

### M2 — `rootle update` (tarball installs only)

- Detect the channel: if the running binary resolves under a brew or
  cargo/mise home, print the channel's own command (`brew upgrade
  rootle`, `cargo install rootle`) and exit 0. Only an install.sh
  binary self-updates.
- The flow is the provider manager's, reused: latest release from the
  GitHub API → platform tarball → **mandatory `.sha256` sidecar
  verification** → staged write → atomic rename over self (unix: the
  running process keeps the old inode) → print old → new.
- `--check` prints without writing. No auto-update, no downgrade UI,
  no Windows (no builds).

### M3 — modeline notice (advisory, never blocking)

- At startup a worker asks the GitHub latest-release API (no token,
  no body, one call), caches the answer 24h under
  `~/.cache/rootle/`, and when the tag is newer than
  `CARGO_PKG_VERSION` the modeline's context chip gains `↑ vX.Y.Z`
  (accent color) — click-free: the notice says `rootle update` in the
  status line once per version.
- Offline / API failure / rate limit: silent — the check must be
  invisible when it fails. `[update] check = false` disables.
- The notice is the same shape brew/cargo users see: they get
  "your channel: brew upgrade rootle" in the one-time status.

## Decisions

- No version polling while running beyond startup; a 24h cache means
  most launches do zero network.
- No in-TUI changelog view — the link is enough (yank-able).
- Integrity is identical to provider installs: no sidecar, no swap.

## Verification

- M2: wiremock-style loopback latest-release fixture in tests;
  self-replace tested against a copy of the test binary in a tempdir
  (never the real one); tampered sidecar refuses.
- M3: render test for the `↑` chip; e2e offline path (no notice, no
  hang) — the fs-provider PTY suite must stay hermetic.
