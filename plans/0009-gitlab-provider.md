# 0009 — GitLab: the first out-of-tree provider

Status: **M0 done** (this document). Milestones flip in the PR that
lands their work, repo convention.

Home for the adapter: **`rootledev/rootle-gitlab`** — Rust, one
binary, stdio NDJSON-RPC, no shared code with rootle (the protocol is
the only contract; that's the point of the exercise).

## Why GitLab, and why this is the real protocol test

GitHub is in-tree and shares types with the app; `fs_provider.py` is a
toy that never leaves localhost. GitLab is neither: a real forge, a
different URL grammar, a different search model, paginated trees, and
auth that can expire mid-session — served entirely through the frozen
wire contract, maintained in a repo that cannot see rootle's source.
Every place the protocol is implicit, under-specified, or accidentally
GitHub-shaped will hurt here. That's the exercise.

## The API mapping (v4 REST, `gitlab.com` defaults, `--instance` override)

| Protocol | GitLab | Notes |
|---|---|---|
| `initialize` | — | capabilities from flags + a lazy probe; startup does NO network (restart obligation) |
| `search/repos` {query} | `GET /projects?search=…` (+ `/groups?search=…` for org results) | full_name = `path_with_namespace`; nested groups mean **multi-slash ids** (F1) |
| `org/repos` {org} | `GET /groups/:id/projects?include_subgroups=true` | org = top-level group; subgroups fold in as full-path repos |
| `repo/tree` {repo} | `GET /projects/:id/repository/tree?ref=:branch&recursive=true&pagination=keyset` | **paginated** (100/page) — adapter aggregates (F4); `truncated` when it stops early |
| `repo/blob` {repo, sha} | `GET /projects/:id/repository/blobs/:sha/raw` | git blob shas are content ids — the cache contract holds as-is |
| `repo/clone_url` | project.`http_url_to_repo` (ssh alt behind a flag) | |
| `repo/web_url` | project.`web_url` + `/-/blob/:branch/:path#L:n` | line fragment grammar differs from GitHub — provider-owned, invisible to rootle ✓ |
| `org/url` | group.`web_url` | |
| `search/code` {q} | `GET /search?scope=blobs` — or `/projects/:id/search`, `/groups/:id/search` for repo:/org: scoping | **availability varies** (F2); returns `startline` + `data` snippet (F7) |

## Findings — what GitLab's reality changes

**F1 — repo ids with multiple slashes.** Nested groups make
`group/subgroup/project` normal. Audit of rootle's parsing sites
(`cli.rs`, `search_popup.rs::split_repo`, `clone_wizard.rs`,
`workers.rs`) found every split is `splitn(2)` — first component =
org, rest = path — so nested ids flow correctly through browsing,
`rootle group/sub/project` CLI args, and the clone wizard's
`<dest>/<group>/<sub/project>` tree. The only wrong text is the
scaffolding skill's "must contain exactly one `/`". Fix the doc, pin
the behavior with a nested-path e2e fixture (R2), declare
multi-slash ids legal in the protocol doc (R4).

**F2 — code search is capability-gated, not universal.** On gitlab.com,
`scope=blobs` works for authenticated users; self-managed instances
need a license (advanced search). The adapter probes lazily on first
`search/code` and downgrades `capabilities.code_search` to false on a
definitive 403 — rootle's UI already degrades on `false`. No wire
change; documented as the canonical capabilities-degradation story.

**F3 — the `q` grammar translation (PROTOCOL SURFACE, per M1).**
`repo:o/r` → project-scoped search endpoint (id resolved + cached);
`org:x` → group-scoped endpoint; `path:` and `extension:` have NO
server-side equivalent → client-side filtering of results before the
HIT_CAP. Translation table lives in the adapter README and is
cross-linked from the protocol doc's grammar note.

**F4 — paginated trees.** The protocol stays unpaginated (the review's
deferred decision stands): the adapter walks keyset pages up to a
byte/entry budget and reports `truncated: true` past it. Guidance
added to the protocol doc (R4): *aggregation is the adapter's job;
`truncated` is the honesty mechanism*.

**F5 — blob size cap is currently provider-specific.** The GitHub
client rejects >1 MiB; the stdio path doesn't enforce anything, so a
provider could push a 500 MB base64 blob through the pipe. Rootle-side
fix (R1): enforce the 1 MiB cap uniformly at the worker boundary for
every provider. Adapters MAY refuse earlier with a `provider`-kinded
error; the client cap is the guarantee.

**F6 — credentials and instance config conventions.** The child
inherits rootle's environment: token via `GITLAB_TOKEN` (or
`ROOTLE_GITLAB_TOKEN`), instance via `--instance URL` argv, read
**lazily on first API call** and cached in-process — the restart
obligations doc already demands exactly this shape. Documented as the
recommended convention (R4); not a wire change.

**F7 — GitLab search returns real line numbers.** `startline` per hit
means `located: true` with genuine line numbers — better than the
GitHub path (fragments without absolute numbers). `matches` carries
the search terms (or `data` snippet trimmed) for the client-side
locate/chip pass.

**F8 — error taxonomy mapping.** 401/403→`auth`; 429 (or
`RateLimit-Remaining: 0`)→`rate_limited` + `retry_after_s` from
`Retry-After`; 404→`not_found`; timeouts→`timeout`; 5xx/DNS→`network`.
Everything else degrades to the message toast, per protocol.

## The adapter repo (`rootledev/rootle-gitlab`)

```
src/main.rs       NDJSON loop: read line → dispatch → reply; NO work at startup
src/api.rs        GitLab client: lazy token, instance url, error mapping (F8),
                  keyset pagination walker
src/handlers.rs   protocol method → api call; q-grammar translation (F3)
src/cache.rs      ~/.cache/rootle/providers/rootle-gitlab/ — sha-keyed trees
                  and blobs (immutable), branch→sha refs; percent-encoded
                  components per the protocol's cache-template note
tests/wiremock.rs offline conformance: every endpoint's shape, paginated
                  trees, 401/403/429 paths, truncated budgets, nested ids
```

Deps mirror rootle's choices (reqwest blocking+rustls, serde, base64,
thiserror) — one binary, `cargo install rootle-gitlab`.

CI: fmt + clippy + wiremock suite on every PR; a `workflow_dispatch`
live job (secret `GITLAB_TOKEN`, read_api scope) running the manual
checklist against gitlab.com; release via rootle's 4-target matrix
pattern, published to crates.io.

## Rootle-side changes (small; land first, M1)

- **R1** uniform 1 MiB blob cap at the worker boundary (all providers)
- **R2** e2e: nested `org/sub/repo` fixture through fs_provider —
  proves id opacity end to end (browse, open, yank URL, clone wizard)
- **R3** scaffolding skill: multi-slash ids legal; drop the
  single-slash requirement
- **R4** protocol doc: aggregation guidance (F4), credential/instance
  conventions (F6), blob cap guarantee (F5), multi-slash ids (F1) —
  documentation only, wire stays v1

## Milestones

| # | Delivers | Status |
|---|---|---|
| M0 | this plan | **done** |
| M1 | rootle-side R1–R4 | **done** |
| M2 | adapter skeleton: loop + initialize + search/repos + org/repos, wiremock CI green | **done** (rootledev/rootle-gitlab, CI + audit green) |
| M3 | browse surface: tree (pagination + truncated), blob, clone_url, web_url, org/url | **done** |
| M4 | search/code: grammar translation, startline locating, capabilities downgrade | **done** |
| M5 | hardening: taxonomy, rate-limit surfacing, lazy auth, disk cache, restart-cheap startup | **done** |
| M6 | live validation vs gitlab.com (token; dispatch-only CI job) + rootle PTY e2e through the adapter | pending |
| M7 | ship: crates.io + 4-target matrix; site providers page + READMEs point at it | pending |

## Testing access

M6 needs a gitlab.com token with `read_api` (+ `read_repository` for
blob raw) scope. CI keeps it a secret on the dispatch job only; local
development reads it from the environment like any user would.

## Non-goals (recorded so they aren't mistaken for oversights)

Wire-level pagination (deferred by standing decision); MR/issue
surfaces; write operations; self-managed TLS quirks (documented flag
only); org-level listing of *subgroups as orgs* (top-level groups
only — subgroup repos fold under their top-level org).
