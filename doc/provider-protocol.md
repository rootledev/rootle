# The rootle provider protocol, v1.3

rootle talks to source-control backends through one seam (`trait
Provider`, `src/provider/mod.rs`). The built-in `github` provider is
the reference implementation; any other system is wrapped as a child
process speaking **NDJSON-RPC 2.0 over stdio** (the LSP model),
implemented in `src/provider/stdio.rs`. The reference adapter is
[`examples/providers/fs_provider.py`](../examples/providers/fs_provider.py),
which serves a local directory of repos — use it as a template and as
documentation-by-example.

![how rootle talks to backends: one seam, github in-tree, anything
else as an NDJSON-RPC stdio child](architecture.svg)

## Transport

- rootle spawns the provider once per app: `command[0]` is the program,
  the rest are arguments. stdin/stdout are pipes; **stderr is
  discarded** unless `[provider] stderr = "inherit"` (v1.2, adapter
  debugging). The child dies with rootle (the handle is held for Drop).
- Each message is one line of JSON on stdin/stdout. Requests carry
  `jsonrpc`, a numeric `id`, `method`, `params`. Replies echo the id
  and carry either `result` or `error`.
- **Transport (v1.2):** a dedicated reader thread owns the child's
  stdout and routes replies by id; requests may be in flight
  concurrently and replies MAY arrive out of order. rootle skips
  (`[provider] timeout_ms`, default 30s): a reply that never comes
  fails that one call with a `timeout`-kinded error — the transport
  and the child stay usable, and the late reply is discarded when it
  finally arrives.
- **Progressive results (v1.3):** a request whose params carry
  `"partial": true` opts into `$/partial` notifications — see
  [Progressive results](#progressive-results-v13). For such requests
  the read deadline is an **inactivity** deadline: every `$/partial`
  or the reply resets it (a provider that keeps streaming never times
  out; a silent one still fails). Non-streaming requests keep the
  per-round-trip deadline.
- **Restart (v1.2):** a closed stdout fails every in-flight call
  ("provider closed its output") and marks the transport dead. The
  next request rebuilds the child with bounded backoff
  (1s → 2s → 5s → 30s cap) and re-runs `initialize` before
  proceeding — only a child that passed the handshake serves
  requests, and the status line notes the restart. Concurrency: at
  most one caller waits out a given rebuild attempt; others either
  ride the validated result or fail fast with the attempt's error.
  `timeout_ms` is a per-round-trip read deadline, not an end-to-end
  bound — a request that triggers a rebuild can additionally wait one
  backoff interval plus one handshake round trip before its own
  attempt.
- **Restart obligations (provider side):** the child may be killed
  and re-`initialize`d an unbounded number of times within one
  session — rootle kills it on exit and restarts it after any death.
  Startup MUST therefore be cheap and idempotent; fetch credentials
  lazily (first use, not at spawn) and cache them — and anything else
  worth keeping — on disk, keyed by the content ids above. In-memory
  state dies with every generation.
- **Reader tolerance (normative, both directions):** unknown fields in
  requests, replies, and results MUST be ignored, and unsolicited
  notifications MUST be ignored. v1.1 additions are additive for exactly
  this reason — `protocol` stays `1`.

```
→ {"jsonrpc":"2.0","id":1,"method":"repo/tree","params":{"repo":"local/alpha"}}
← {"jsonrpc":"2.0","id":1,"result":{"entries":[…],"truncated":false,"branch":"main"}}
← {"jsonrpc":"2.0","id":2,"error":{"code":1,"message":"unknown repo 'local/x'"}}
```

## Handshake

First request after spawn:

```
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{
     "protocol":1,
     "cache_bytes":536870912,
     "cache_dir":"/home/u/.cache/rootle/providers/gitlab"
   }}
← {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"name":"fs",
     "capabilities":{"orgs":true,"code_search":true},
     "cache":{"bytes":218}}}
```

`protocol` must be `1` (anything else aborts stdio setup and rootle falls
back to the GitHub provider with a warning). `name` is optional and is
shown as `stdio:<name>`. `capabilities` is optional and defaults to
everything enabled; the UI degrades on `false` (`orgs`, `code_search`).

**Cache budget (advisory, v1.2):** `cache_bytes` is the user's
`[cache] max_mb` budget in bytes and `cache_dir` is this provider's
cache subtree — rootle passes both at every initialize (spawns and
respawns alike). Providers that cache on disk SHOULD respect the
budget — evict least-recently-used entries past it — so one knob in
`:settings` governs every backend and they all feel native. It is
advisory: a provider may ignore it, and rootle never reaches into the
subtree. The reply may carry `cache: {"bytes": N}` (current subtree
size) — rootle shows it in `:settings` next to the provider row.

## Methods

Repos are opaque `"group/project"` strings; the UI never parses them.
Optional/missing fields noted per method; everything else is required.

| Method | Params | Result |
|---|---|---|
| `initialize` | `{"protocol":1}` | `{protocol, name?, capabilities?}` |
| `search/repos` | `{"query"}` | `{"items":[{"full_name":"o/r"} \| {"org":"o"}]}` |
| `org/repos` | `{"org"}` | `{"repos":[name, …]}` |
| `repo/tree` | `{"repo"}` | `{"entries":[…], "truncated":bool, "branch":"main"}` |
| `repo/blob` | `{"repo","sha"}` | `{"bytes_b64":"…"}` |
| `repo/clone_url` | `{"repo"}` | `{"clone_url":"…"}` |
| `repo/web_url` | `{"repo","path","branch","line"}` | `{"url":"…"}` |
| `org/url` | `{"org"}` | `{"url":"…"}` |
| `search/code` | `{"q"}` | `{"items":[…], "truncated"?:bool}` |

Details:

- `search/repos` — `items` defaults to `[]`. An item with `full_name`
  is a repo; else `org` is an org; items with neither are dropped.
- `org/repos` — `repos` defaults to `[]` (repo names, not full paths).
- `repo/tree` — one entry per path, recursive over the default branch.
  Pagination is the adapter's job: backends that page (GitLab keyset,
  GitHub none) are aggregated up to an adapter-chosen budget, with
  `truncated: true` past it — the wire stays unpaginated:
  `{"path":"src/main.rs","type":"blob","sha":"…","size":123}` where
  `type` is `"blob"` or `"tree"` (`"tree"` renders as a directory);
  `size` is optional and blobs only. `entries` defaults to `[]`,
  `truncated` to `false`, `branch` to `"main"`.
- `repo/blob` — `bytes_b64` is standard-base64 file content. rootle
  refuses blobs over 1 MiB at its boundary regardless of provider
  (preview-pane policy); adapters MAY refuse earlier with a
  `provider`-kinded error.
- `repo/web_url` — build the browser URL for a repo root (`path` empty),
  a path (tree/blob grammar is the provider's), appending a line
  fragment when `line` is a number (`line` is JSON `null` when absent;
  `branch` may be empty — the provider resolves it).
- `repo/clone_url` — a URL `git clone` accepts.
- `search/code` — `q` is the full query with qualifiers (`repo:`,
  `org:`, `extension:`, `path:`). Items: `{"repo","path","sha","branch",
  "matches":[str,…]}`; `sha` defaults to `""`, `branch` to `"main"`,
  `matches` to `[]`. `matches` are matched substrings — the UI locates
  them in the blob to compute real line numbers and previews. Optional
  per-item `located` (bool, default `true`): `false` means the provider
  knows its index is stale for this hit — the UI shows a `stale` chip
  instead of line numbers until client-side locating self-heals it.
  Optional top-level `truncated` (bool, v1.2, default `false`): `true`
  means the provider capped its own result set — the UI marks the
  results as clipped so a complete set is distinguishable from a cut
  one.
- **Progressive search (v1.3, plans/0011):** rootle sends
  `search/code` with `"partial": true` and renders `$/partial` batches
  as they arrive — see the section below.

## Content ids

Every `sha` is an opaque **content id**: it MUST change when content
changes. rootle's cache is content-keyed and immutable — trees live at
`~/.cache/rootle/trees/<sha>.json`, blobs at `blobs/<ab>/<rest>`, and are
never invalidated, only evicted. A provider that reuses a sha for
different bytes will show stale content. `fs_provider.py` hashes blob
bytes with sha256 (directories hash their path — they have no content).

## Cancellation (advisory)

rootle may send an LSP-style cancellation **notification** (no reply)
for a request it no longer needs — a superseded search, a context
fetch whose hit was scrolled past:

```
→ {"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":7}}
```

Advisory only: the reply may still arrive and is handled normally
(rootle's replies are id-matched and generation-guarded, and blob
content is sha-keyed, so late work is never wrong). Providers MAY
ignore it — cache-backed adapters lose nothing; quota-paying or
long-running backends SHOULD use it to skip work. Cancels for unknown
or completed ids are ignored.

## Progressive results (v1.3)

Long-running requests stream their result instead of arriving as one
block (plans/0011; the shape follows LSP's `partialResultToken` +
`$/progress` design, keyed by request id — and Sourcegraph's stream
API validated the event split for search specifically):

```
→ {"jsonrpc":"2.0","id":7,"method":"search/code","params":{"q":"render","partial":true}}
← {"jsonrpc":"2.0","method":"$/partial","params":{"id":7,"items":[ …hit, …hit ]}}
← {"jsonrpc":"2.0","method":"$/partial","params":{"id":7,"items":[ …hit ]}}
← {"jsonrpc":"2.0","id":7,"result":{"items":[],"truncated":false}}
```

Rules:

- `"partial": true` in a request's params = the client consumes
  `$/partial` notifications carrying that request's `id`. rootle
  sends it on every `search/code`.
- `items` are the method's own result-item shape, **append-only**.
  No overwrite mode: a hit the provider later knows better is
  re-sent as another item and the client folds by file identity;
  `located: false` + client-side locating self-heal placements.
- Line order on the single pipe: all `$/partial` for an id precede
  that id's reply. After the reply, no more for that id.
- When the provider streamed, **the reply is metadata-only** —
  `items` empty, `truncated` authoritative. Without `partial` in
  params the reply carries everything (unchanged v1.2 behavior).
- The deadline is per-inactivity while streaming (see Transport).
- `$/cancelRequest` (v1.1) stops the stream; the reply may still
  arrive and is handled normally.
- Child death mid-stream: partials already rendered stay; the request
  itself fails ("provider closed its output") per the restart rules.
  Rootle marks the set incomplete rather than discarding it.

The reference adapter (`fs_provider.py`) streams `search/code` per
repo; the in-tree GitHub provider streams per REST page (100/page,
3-page budget, `truncated` past that).


## Errors

Reply with a JSON-RPC `error` object instead of `result`:

```
← {"jsonrpc":"2.0","id":3,"error":{"code":1,"message":"no blob abc in local/alpha"}}
```

The `message` becomes the `Err` the UI shows as a one-line status/toast
(`code` is ignored). A reply with neither `result` nor `error` fails
with "provider reply without result". `fs_provider.py` wraps every
handler exception this way.

**Kinds (v1.1, optional).** Errors may carry a semantic kind in
`data.kind` — an open string enum the UI maps to precise handling:

```
**Kinds (v1.1, optional).** Errors may carry a semantic kind in
`data.kind` — an open string enum the UI maps to precise handling:

Defined kinds: `auth`, `rate_limited` (optional `retry_after_s`
seconds), `not_found`, `network`, `timeout`, `provider` (internal).
Unknown kinds degrade to the message toast — never error on them.
`code` stays any positive int of the provider's choosing (the JSON-RPC
standard `-32xxx` codes remain reserved for protocol-level errors).

**Rendering (v1.2 — kinds are wired, not just parsed):** `auth` shows
the message with a refresh-credentials hint; `rate_limited` shows a
throttled notice with the backoff seconds. rootle also *generates*
kinds host-side: `timeout` when the read deadline fires, `provider`
when the child dies.

## Configuration

`~/.config/rootle/config.toml` (or a file passed to `rootle --config`):

```toml
[provider]
kind = "stdio"
command = ["python3", "/path/to/fs_provider.py", "/path/to/code"]
timeout_ms = 30000      # v1.2: per-request read deadline (default 30s)
# stderr = "inherit"    # v1.2: pass child stderr through. Recognized:
                       # "inherit" | "null" (default); anything else
                       # warns on the status line and discards.
# name = "ghes"        # short display name for the modeline's forge
                       # chip; defaults to the handshake's self-reported
                       # name.
[ui]
# border = "plain"     # pane/popup corner style: "plain" (default) |
                       # "rounded" | "thick" | "double". Unknown values
                       # fall back to plain.
# nerd_font = false    # Nerd Font glyphs in the modeline (powerline
                       # arrows + forge icons); false keeps unicode
                       # fallbacks (❯, text-only chips).
`kind = "github"` (the default) uses the built-in provider. An empty
command, a failed spawn, or an unknown kind falls back to GitHub with
a warning on the status line — misconfiguration never blocks startup.

**Credential & instance conventions (recommended, not parsed):** the
child inherits rootle's environment — read tokens from a
backend-specific env var (`GITLAB_TOKEN`, …) **lazily, on first use**,
never at startup (the restart obligations above); instance/hostname
selection belongs in argv (`--instance URL`). The fs reference adapter
and the GitLab adapter (`rootle-gitlab`) both follow this shape.

Try the reference adapter against a directory of repos:

```
python3 examples/providers/fs_provider.py ~/code   # serves ~/code/* under "local"
rootle --config provider.toml                          # with the [provider] block above
```

The e2e suite drives the full TUI through this protocol against
`fs_provider.py` (`e2e/test_provider.py`) — offline proof of the whole
path: search, tree walk, blob preview, code search.

## In-tree providers

For backends that should live in the binary, implement `trait Provider`
(`src/provider/mod.rs`): `name`, `capabilities`, and the calls above
(`search`, `org_repos`, `fetch_tree`, `fetch_blob`, `search_code`,
`clone_url`, `web_url`, `org_url`, plus optional `default_orgs` for
cold-start suggestions), then register it in `provider::build`. The
same content-id and opaque-repo rules apply.

Scaffolding: `skills/rootle-provider/SKILL.md` (in this repo) walks
through building a provider — capability questionnaire, adapter
skeleton, and a conformance test suite that gates integration.

## Disk caches

Providers that cache on disk must not write into `~/.cache/rootle/`
directly — that root belongs to the TUI (`edit/` scratch). Use a
provider-scoped subtree (initialize passes the resolved path as
`cache_dir`, and the user's size budget as `cache_bytes` — see the
handshake):

```
~/.cache/rootle/providers/<name>/…
```

The GitHub provider is the reference design
(`~/.cache/rootle/providers/github/`): content-addressed blobs and trees
(`blobs/<ab>/<rest>`, `trees/<sha>.json` — immutable, never
invalidated, only evicted), mutable ref→sha mappings revalidated with
ETag (`index/refs/<org>/<repo>/<branch>` — a `304` is free), atomic
tmp+rename writes, LRU eviction by mtime at startup, orphan sweep
(trees not referenced by any ref, blobs not referenced by any live
tree). If your backend can produce the same shape, copy it.
