# 0011 — Progressive search results (protocol v1.3)

Status: **done** (flipped in the landing PR)

## Problem

`search/code` is one blocking call under a 30s deadline, and the UI
caps what it keeps at 25 hits (`HIT_CAP`, plans/0008 §4). On a corpus
of any size the first screen of results waits for the last, and a
search is a sample. Enterprise feedback (internal GHE adapter team)
asked for `offset`/`limit` pagination; offset cursors are polling-shaped
and would pre-empt the streaming question this protocol already faces
(`repo/clone` progress, org enumeration).

## Prior art

- **LSP 3.15+** is the direct ancestor of this transport and solved
  exactly this (microsoft/language-server-protocol#786 →
  `partialResultToken` + `$/progress`): client opts in via a token in
  the request; one notification route carries per-request-shaped
  partial payloads; **the final reply carries no items once partials
  were sent** (no double-send, no reconcile ambiguity); on error
  mid-stream the results so far stay and the error surfaces
  separately.
- **Sourcegraph's stream API** (`/.api/search/stream`) validates the
  event split for search specifically: `matches` events stream, a
  terminal `done` carries counts/alerts — same shape over SSE.

## Design — protocol v1.3

`$/partial` notifications, keyed by request id (no separate token —
our `$/cancelRequest` already addresses ids, and request-scoped is the
only scope we need; a server-initiated `$/progress` stays available
for future work-done progress):

```
→ {"jsonrpc":"2.0","id":7,"method":"search/code","params":{"q":"render","partial":true}}
← {"jsonrpc":"2.0","method":"$/partial","params":{"id":7,"items":[ …hit, …hit ]}}
← {"jsonrpc":"2.0","method":"$/partial","params":{"id":7,"items":[ …hit ]}}
← {"jsonrpc":"2.0","id":7,"result":{"items":[],"truncated":false}}
```

Rules:
- `"partial": true` in params = the client consumes `$/partial` for
  that request's id. rootle sends it on every `search/code`.
- Items are the method's own result-item shape, append-only. No
  overwrite mode (LSP declined it too; stale hits self-heal via
  `located: false` + lazy client-side locating).
- Line order on the single pipe: all `$/partial` for id N precede N's
  reply. After the reply, no more for N.
- When the provider streamed, the reply is metadata-only (`items`
  empty, `truncated` authoritative). Without `partial` in params the
  reply carries everything (unchanged).
- **Deadline change:** for streaming requests the read deadline is an
  inactivity deadline — any `$/partial` or the reply resets it.
  Non-streaming requests keep the per-round-trip deadline.
- `$/cancelRequest` stops the stream; late partials for a cancelled id
  are dropped by the existing generation guards.
- Child death mid-stream: partials already rendered stay; the request
  fails "provider closed its output" (v1.2 restart rules unchanged).

## Design — rootle internals

- `Provider::search_code_progressive(q, on_hits)` — `on_hits` may fire
  from any thread, any number of times, strictly before the call
  returns; the returned `SearchCodeResult` is metadata-only when the
  provider streamed. Default impl: run `search_code`, emit one batch.
  Every provider therefore streams; ones with page-shaped backends
  stream page-by-page.
- GitHub: page `search/code` (`per_page=100`, up to 3 pages = 300
  hits, `truncated` past that; GitHub itself caps at 1000).
- stdio: transport gains `exchange_with_partials` — the reader thread
  routes `$/partial` to the pending slot's channel; the waiting worker
  pumps each through the sink and treats any message carrying a
  `method` field as a delta, resetting the deadline.
- Worker: sink sends `AppEvent::GlobalSearchDelta { gen_id, hits }` on
  the existing event channel; `search_gen` drops stale batches.
- View: `append_hits` merges same-file hits (region union + count
  badge) and appends the rest; render cap `RENDER_CAP = 500` (beyond:
  count climbs, title says clipped). The final metadata event only
  clears `pending` and sets `clipped` — the accumulated set stands.
- `HIT_CAP` (25) is gone; eager blob-locate stays capped at 8
  (`PREVIEW_CAP`), lazy per-hit context covers the rest (plans/0006).

## Consequences

- The feedback team's pagination ask is answered structurally: no
  cursor, no load-more key, no keymap growth — results arrive until
  the provider's own budget, and `truncated` still distinguishes
  complete from clipped.
- `repo/clone` progress and org-enumeration streaming now have a
  proven route (`$/partial` for request-scoped data, `$/progress`
  reserved for work-done when clone lands).
- Breaking for external adapters only in the soft sense: unknown
  notifications were already tolerated, and a v1.3 provider streaming
  to an old rootle shows partials then an empty final — the
  conformance suite gates v1.3 support.

## Verification

- stdio unit tests (fake_provider_child): ordered deltas before the
  reply, metadata-only final, dead child mid-stream keeps partials and
  fails the request, cancel drops late batches by generation.
- Render tests: delta append + merge, empty final keeps accumulated
  set and applies `clipped`, stale `gen_id` dropped.
- e2e: the fs provider (now streaming) drives the full flow; docker
  gates green.
