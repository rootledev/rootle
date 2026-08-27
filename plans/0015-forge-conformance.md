# 0015 — forge-conformance: the canonical provider conformance suite

Status: **in progress** (2026-08-27): suite live at
`rootledev/forge-conformance` (37 cases, CI green); rootle CI job +
skill wiring in this PR; rootle-gitlab wired (#4, 37/37 after the
adapter grew v1.3 streaming); rootle-bitbucket pending

## Problem

Today every scaffolded provider gets a *copy* of a conformance test
file (`skills/rootle-provider` generates `test_conformance.py`).
Copies drift: the protocol moves (v1.1 → v1.3 added `located`,
`$/partial`, `line`, `file_search`, `index`, `limit`), each adapter's
copy doesn't, and nothing distinguishes "my copy is old" from "my
adapter is wrong". The protocol needs one canonical, versioned,
executable gate — every edge case and gotcha as a numbered case that
any adapter runs against and must pass to integrate. This is the
gripfetch-conformance pattern: one repo, the gotchas are the test
matrix, adapters implement against it.

Our three providers (fs reference, rootle-gitlab, rootle-bitbucket)
must pass it in their own CI — the suite is also the regression gate
for future protocol revisions.

## The repo

`rootledev/forge-conformance` — a runner plus a fixture, no adapters
inside:

```
forge-conformance/
  run                     # entry: python3 run -- <provider-command...>
  cases/                  # one module per case group (below)
  fixture/                # the canonical mini-backend dataset
  FIXTURES.md             # what the fixture encodes and why
  README.md               # usage, per-case index, spec citations
  .github/workflows/ci.yml
```

- **Runner**: Python stdlib + pytest (least common denominator for
  adapters in any language). `PROVIDER="my-adapter --flag" python3 -m
  pytest` or `python3 run -- /path/to/adapter`. Spawns the adapter
  once per case group, speaks the wire directly (no TUI, no rootle
  binary required), and reports per-case pass/fail with the
  **spec section the case encodes** in the failure output.
- **Fixture**: a canonical mini-backend the adapter serves (two repos:
  `alpha` — a rust-ish tree with unicode filenames, an empty file, a
  >1 MiB blob, a binary file, a nested `src/`; `beta` — a notes-only
  repo). Content is deterministic so sha256 ids are computable by the
  suite and comparable against what the adapter emits.
- **Case IDs**: stable numbered cases (`FC-021`) so failures are
  citable across adapters, docs, and bug reports — like Atlassian's
  CHANGE numbers, the case never renames.

## The case matrix (every gotcha, one case)

Grouped; each case cites the spec section.

**Handshake (v1)** — FC-001 protocol=1 echoes; FC-002 name present or
absent; FC-003 capabilities shape; FC-004 icon is a name or single
glyph or absent; FC-005 cache budget params tolerated when absent.

**Content ids (the contract that breaks caches)** — FC-010 same bytes
→ same sha across two calls; FC-011 changed bytes → different sha;
FC-012 different content → different sha (no path-keyed collisions);
FC-013 sha stability across respawns (kill child, respawn, same blob).

**Trees & blobs** — FC-020 recursive walk covers every fixture path;
FC-021 dir vs blob typing; FC-022 truncated flag past a cap; FC-023
blob >1 MiB refused (any error, `provider` kind preferred); FC-024
binary bytes served raw, base64 valid; FC-025 tree at missing repo →
`not_found`.

**Search (v1.3)** — FC-030 path-only hit: empty `matches` legal, must
render; FC-031 `line` is 1-based and equals the fixture's real line;
FC-032 `located:false` tolerated; FC-033 `index.as_of` shape when
present; FC-034 `file_search` inherits `code_search` when absent.

**Streaming (v1.3, the strictest group)** — FC-040 every `$/partial`
carries the request's id; FC-041 all partials precede the reply, no
partial after it; FC-042 reply is metadata-only when streamed (`items`
empty, `truncated` authoritative); FC-043 **inactivity deadline**: a
streamer emitting one batch every 1.2s must NOT trip a 2s request
timeout (the deadline resets per batch); FC-044 a batch larger than
the client's render budget is accepted (client clips, adapter must
not crash); FC-045 stream stops cleanly on `$/cancelRequest`
(advisory: subsequent partials may arrive but MUST NOT error).

**Lifecycle (restart obligations)** — FC-050 kill the child mid-
session: respawn is cheap and idempotent (initialize completes again
within the request timeout); FC-051 initialize re-runs every
generation with the same cache params; FC-052 no network I/O during
initialize (credentials are lazy — the fixture's API token is unset
during FC-051 and set later); FC-053 unknown fields in requests are
ignored (reader tolerance, both directions); FC-054 unsolicited
notifications ignored.

**Errors (taxonomy)** — FC-060 `auth` for credential failures; FC-061
`rate_limited` with `retry_after_s` when applicable; FC-062
`not_found` for missing repo/blob; FC-063 unknown kinds from the
adapter are tolerated by the client-side mapper (suite asserts shape,
not kind set).

**Bounded compute (v1.4 advisory)** — FC-070 honoring `limit` stops at
~N and sets `truncated: true` (the pinned decision: limit-stop means
exactly a provider's own cap).

**Icons (v1.3)** — FC-080 `icon` is absent, a builtin name, or a
single scalar; multi-char strings are rejected by shape validation.

## Skill wiring (in this plan's implementation)

- `skills/rootle-provider/SKILL.md` gains the gate as **the** entry
  point: after the grill, the scaffold points at
  `rootledev/forge-conformance` and the per-adapter generated
  `test_conformance.py` is removed — adapters run the canonical suite,
  not a copy. The skill's description line updates to name it.
- The suite's README is linked from `doc/provider-protocol.md` (the
  scaffolding pointer) and the site's providers/your-forge page.
- `rootledev/fs_provider.py` runs the suite in rootle's own CI (new
  job): protocol changes must keep the reference adapter green.
- rootle-gitlab and rootle-bitbucket add a suite job to their
  workflows (wiremock fixtures map to the canonical fixture).

## Verification

- The suite runs green against fs_provider.py on day one (it is the
  reference — a red suite against the reference means the suite is
  wrong, not the adapter).
- Each of FC-040..045 verified against the stdio fake-provider modes
  first (rootle's own unit fixtures) so case semantics match what
  rootle actually enforces, not what the author thinks it enforces.
- One deliberately-nonconforming fixture adapter (skips FC-013, FC-043)
  fails exactly those cases and no others — the gate catches what it
  claims to catch.
