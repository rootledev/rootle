# 0010 — Provider manager: install, upgrade, use

Status: **planned** — design from a survey of gh extension, krew, helm
plugins, cargo, mise, asdf, tpm, docker, zellij, deno, uv (research
brief in the PR).

The shape users expect, in one line:

    rootle provider install rootledev/rootle-gitlab
    rootle provider use gitlab
    rootle provider upgrade --all

## Why

Plans/0009 shipped the first out-of-tree provider; today installing it
means `cargo install` (a Rust toolchain on the user's machine) plus
hand-editing config.toml. The survey says the ecosystem has converged:
`install / list / upgrade / remove` verbs, `owner/repo` references,
versioned stores with receipts, checksums mandatory, notice-only
updates. rootle already distributes itself exactly the way providers
will distribute (GitHub release tarballs, 4 targets, sha256 sidecars)
— the same asset contract serves both.

## CLI surface (clap subcommand on the existing binary)

```
rootle provider install <ref> [--pin]        # ref: gitlab | owner/repo[@tag] | https://github.com/...
rootle provider install --path <dir> <name>  # dev: local binary, no network
rootle provider list [--json]                # name, version, pinned, source, ACTIVE
rootle provider update [name]                # refresh latest-known tags (krew's non-mutating update)
rootle provider upgrade [name | --all] [--dry-run] [--force]
rootle provider pin <name> [tag] / unpin <name>
rootle provider remove <name> [--purge-cache]
rootle provider use <name> [-- argv...]      # mise's verb: writes config, the single active provider
```

- **`upgrade` for the binary swap, `update` for metadata** — krew's
  split; gh and uv agree on `upgrade`.
- **`use` for activation** — rootle has exactly one active provider;
  `use gitlab -- --instance https://gitlab.example.com` captures
  provider argv (plans/0009 F6) into config.
- Bare name resolves through the **`rootle-<name>` repo convention**
  (gh's `gh-` prefix rule): `rootle provider install gitlab` →
  `rootledev/rootle-gitlab`. No central index in v1 (rejected:
  maintenance burden at single-digit provider counts; discovery is the
  site's providers page + GitHub topic — an index can be added later
  without grammar changes, exactly as krew did).

## On-disk layout (XDG; config declares, the store installs)

```
~/.local/share/rootle/providers/<name>/<version>/rootle-<name>   # versioned binaries
~/.local/share/rootle/providers/<name>/current -> <version>/     # relative symlink
~/.local/state/rootle/providers/<name>.toml                      # receipt: repo, tag, sha256, pinned
```

`provider use` rewrites the **existing** `[provider]` block (kind =
"stdio", command = […/providers/gitlab/current/rootle-gitlab, …argv])
via `Config::save()`. `provider::build()` is untouched; hand-edited
configs keep working; the manager is install-time only, never a
runtime dependency of the TUI.

## Install/upgrade mechanics

1. resolve ref → release (latest or pinned tag)
2. pick the asset by suffix match on the 4-target matrix naming
   (`rootle-<name>-<version>-<target>.tar.gz`; tolerate
   `linux-x86_64` and bare-binary variants — gh's asset matcher is the
   model)
3. download to temp staging
4. **verify sha256 against the `.sha256` sidecar — mandatory** (krew
   rule; a missing checksum is a failed install, not a warning)
5. extract → chmod 0755 → move into `<name>/<version>/`
6. write the receipt LAST (a failed step leaves no phantom install)
7. atomically re-point `current`

Never overwrite the running binary (versioned dirs + pointer swap);
per-provider failures never abort `upgrade --all` (krew/gh batch
semantics); first install prints the gh-style trust notice naming
owner/repo.

## Update semantics

Default: track the latest release tag. `pin`/`@tag` freezes (upgrade
skips pinned without `--force`). `update` is the non-mutating
freshness check; `upgrade --dry-run` prints old → new. In the TUI, a
quiet check at most once per 24h rides the existing
`take_notice()`/status-line plumbing — "gitlab v0.2.0 available ·
:provider upgrade" — gated by `[provider] update_check = false` to
disable. **No tool surveyed auto-upgrades; neither will we.**

## TUI surfacing

**Settings popup — new `providers` section** (the popup already has a
provider section; this extends it): installed providers as rows —
`▸ gitlab   v0.1.0   rootledev/rootle-gitlab   [ACTIVE]` — with the
ACTIVE row carrying the selection background, `pinned` shown as a ` pin`
chip. Keys: `Enter` = use (rewrites config; status-line "restart to
apply" — the provider switch itself stays a restart, matching the
`provider changes note a restart` row descriptions already in the
settings popup), `u` = upgrade (async worker; progress and errors ride
the existing toast/status plumbing — never block the event loop on
network), `x` = remove (confirm). A one-line install field at the
bottom of the section takes an `owner/repo` ref (the VimInput
component is already shared).

**Modeline** — the provider's self-reported name (the `name` field
from `initialize`; today "stdio:gitlab") joins the modeline context
area with a compact glyph:

    INSERT   ⛁ gitlab · repo:rootledev/rootle …    n/N match …

`⛁` (or `⧉`) reads as "backend store" without demanding space; the
std provider reports `github` so the chip is always present and
truthful — the name comes from the wire, not config, so a provider
cannot masquerade after install. Strip the `stdio:` prefix at the
display layer (the transport keeps it for logs where disambiguation
matters).

## Non-goals (recorded)

Central index (v1); script/git-clone providers (providers are static
binaries by contract — plans/0009); provider lifecycle hooks (helm
platformHooks); spawn-time permission prompts (docker) — credentials
inherit via env per the protocol's conventions; in-TUI registry
browsing (the install field covers it).

## Milestones

| # | Delivers | Status |
|---|---|---|
| M0 | this plan | **done** |
| M1 | manager core: resolve/download/verify/store/receipts + install/list/remove | pending |
| M2 | update/upgrade/pin + `use` config writer | pending |
| M3 | clap surface + `--json` for scripting | pending |
| M4 | settings providers section (list/use/upgrade/remove + install field) | pending |
| M5 | modeline provider chip + 24h update notice | pending |
| M6 | rootle-gitlab installed end-to-end via `rootle provider install gitlab` (dogfood) | pending |
| M7 | docs: site providers page install block, README, protocol doc pointer | pending |
