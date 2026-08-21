# 0006 — Provider protocol v1.1 + preview line cursor

Status: in progress (spec locked with the integration team; this plan
is the contract for the first implementation slice).

## 0. Context

The work integration of rootle flagged four protocol refinements, plus
one TUI-only feature (preview line cursor) that rides the same
milestone. Decisions below are locked with the integrators:

1. Per-hit context: **reuse `repo/blob` lazily** — no new method.
2. Cancel: **advisory `$/cancelRequest` notification** (LSP name).
3. Staleness: **per-item `located` bool** on `search/code`.
4. Errors: **`data.kind` open string enum** + free numeric codes.
5. Preview line cursor: **TUI-only** — `repo/web_url` already takes
   `line`; no protocol change.

Versioning: everything is additive and ignorable in both directions, so
the handshake stays `protocol: 1`. The reader-tolerance rule (unknown
fields and unsolicited notifications MUST be ignored) becomes explicit
normative text — it is already the de facto behavior.

## 1. Per-hit context via lazy `repo/blob` (UI contract)

Today `run_view_search` eagerly locates previews for the first
`PREVIEW_CAP = 8` hits (cache-first); the rest render as bare paths.
New behavior:

- When the results cursor lands on a hit that has a sha but no preview
  lines, the view emits `Action::LoadHitContext`; App spawns a worker
  that fetches `repo/blob` and runs the same `locate_matches` folding
  used for the eager path.
- Result returns as `HitContextLoaded`, generation-guarded by `view_gen`
  (stale fetches for closed/superseded views are dropped like search
  results).
- Replacing a still-pending context fetch (cursor moved on) sends an
  advisory cancel first (§2).
- Cost model: the blob cache is content-keyed and immutable, so a hit's
  context is fetched at most once per (repo, sha) — repeat selections,
  later searches, and the editor-open path all hit the cache.

Rejected: a `search/context` method returning server-trimmed windows.
It duplicates provider-side logic, sidesteps the content cache, and its
only win (bandwidth on huge blobs) has no demonstrated need. If a
hosted provider hits that wall, spec it as an OPTIONAL method later.

## 2. Advisory cancel — `$/cancelRequest`

Wire format (notification — no reply):

```
→ {"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":7}}
```

Semantics:

- **Advisory only.** The reply may still arrive and MUST be handled as
  if the cancel was never sent (rootle: results are id-matched and
  generation-guarded, so late replies are dropped or applied — both
  safe; content is sha-keyed, so a late blob is never wasted).
- Providers MAY ignore it (cache-backed adapters lose nothing).
  Quota-paying or long-running backends SHOULD use it to skip work.
- A cancel for an unknown or completed id is ignored.

Client implementation (this plan): requests serialize under the io
mutex, so at most one id is genuinely in flight. `StdioProvider` tracks
it in an atomic; `advise_cancel()` writes the notification under the
stdin lock (split from the reader lock — notification writes must not
queue behind a blocked reader). Sent when: a new view search supersedes
an in-flight one, a new search-popup query supersedes, and a context
fetch replaces a pending one. Reference providers tolerate the
notification (fs_provider ignores request-less lines — conformance
case).

## 3. `located` on `search/code` items

Per-item optional bool, default `true`:

```
{"repo":"o/r","path":"src/a.rs","sha":"…","matches":["…"],"located":false}
```

- `true` / absent — match placement is verified against the content at
  `sha` (or the UI can locate client-side as it does today).
- `false` — the provider knows its index is stale for this hit
  (indexed bytes ≠ blob at sha, or no sha to verify against). The UI
  renders a `stale` chip instead of line numbers.
- Self-heal: once the blob is located client-side (eager preview or
  lazy context §1), the chip clears. "Not yet fetched" needs no flag —
  that is the UI's own lazy state.

## 4. Error taxonomy

Errors keep the JSON-RPC shape; semantics ride in `data.kind`:

```
← {"jsonrpc":"2.0","id":9,"error":{"code":1,
     "message":"rate limited (reset in 37s)",
     "data":{"kind":"rate_limited","retry_after_s":37}}}
```

- `code`: numeric, as JSON-RPC requires. `-32600/-32601/-32700` etc.
  stay reserved for protocol-level errors; app errors use any positive
  int, provider-chosen (opaque to the UI).
- `data.kind`: **open string enum** — `auth`, `rate_limited`,
  `not_found`, `network`, `timeout`, `provider`. Unknown kinds degrade
  to today's message toast; nothing breaks.
- Optional structured extras per kind (`retry_after_s` for
  `rate_limited`) — forward-compatible by the tolerance rule.
- UI mapping (later slice): `auth` → setup hint, `rate_limited` →
  cooldown on the search field, `not_found` → quiet per-row state,
  `network`/`timeout` → retryable toast. This plan only defines the
  wire shape; the rendering lands with the first provider that emits
  kinds.

Why strings over a numeric registry: self-documenting for provider
authors, greppable in logs, and extensible without a centralized
versioned table. The numeric `code` field remains free-choice because
JSON-RPC mandates it, not because the UI reads it.

## 5. Preview line cursor (TUI-only)

`J/K` in BROWSE mode move a line cursor through the preview pane
(scroll follows; previously J/K scrolled by 3). The preview border
shows a `line/total` readout; the cursor line carries the selection
tint. `␣ y` yanks the URL for the file **at the cursor line**
(`repo/web_url` with `line = Some(n)` — the parameter has existed since
v1). Dir summaries and binary placeholders have no cursor; yank there
keeps today's line-less URL. Search-view yank already anchors the hit's
match line.

## 6. Milestones

1. **Spec + protocol-send + TUI slice (this PR)**
   - plans/0006 + provider-protocol.md v1.1 section.
   - `$/cancelRequest` sender in stdio.rs; trait `advise_cancel`;
     supersession wiring; fs_provider tolerance + conformance note.
   - `located` plumbed to a `stale` chip; lazy per-hit context beyond
     PREVIEW_CAP; preview line cursor + line-anchored yank.
2. **Kind rendering** — when a real provider emits kinds: error-kind
   mapping in stdio.rs + status-line/row states.
3. **(contingent) `search/context`** — only if a hosted provider
   demonstrates bandwidth pain on `repo/blob`.

## 7. Verification

- Unit: preview cursor clamping/readout; cancel notification JSON;
  `located` deserialization (absent, true, false).
- Render (TestBackend): readout renders; stale chip renders; cursor
  line tint; no residue.
- e2e (fs provider): J-moves readout; line-anchored yank via
  `ROOTLE_CLIPBOARD`; lazy context materializes on a bare hit beyond
  the eager cap; rapid hit movement (cancel storm) leaves the provider
  alive and the view consistent.
- Docker gate + full matrix per the PR skill.
