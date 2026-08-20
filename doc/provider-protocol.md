# The ghx provider protocol, v1

ghx talks to source-control backends through one seam (`trait
Provider`, `src/provider/mod.rs`). The built-in `github` provider is
the reference implementation; any other system is wrapped as a child
process speaking **NDJSON-RPC 2.0 over stdio** (the LSP model),
implemented in `src/provider/stdio.rs`. The reference adapter is
[`examples/providers/fs_provider.py`](../examples/providers/fs_provider.py),
which serves a local directory of repos — use it as a template and as
documentation-by-example.

## Transport

- ghx spawns the provider once per app: `command[0]` is the program,
  the rest are arguments. stdin/stdout are pipes; **stderr is
  discarded**. The child dies with ghx (the handle is held for Drop).
- Each message is one line of JSON on stdin/stdout. Requests carry
  `jsonrpc`, a numeric `id`, `method`, `params`. Replies echo the id
  and carry either `result` or `error`.
- Requests are serialized under a mutex and matched by id; ghx skips
  non-JSON lines and replies whose id doesn't match (notifications are
  tolerated). A closed stdout fails every call ("provider closed its
  output"). There is no restart policy in v1 — a dead child surfaces
  per-call errors as status-line toasts.

```
→ {"jsonrpc":"2.0","id":1,"method":"repo/tree","params":{"repo":"local/alpha"}}
← {"jsonrpc":"2.0","id":1,"result":{"entries":[…],"truncated":false,"branch":"main"}}
← {"jsonrpc":"2.0","id":2,"error":{"code":1,"message":"unknown repo 'local/x'"}}
```

## Handshake

First request after spawn:

```
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":1}}
← {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"name":"fs",
     "capabilities":{"orgs":true,"code_search":true}}}
```

`protocol` must be `1` (anything else aborts stdio setup and ghx falls
back to the GitHub provider with a warning). `name` is optional and is
shown as `stdio:<name>`. `capabilities` is optional and defaults to
everything enabled; the UI degrades on `false` (`orgs`, `code_search`).

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
| `search/code` | `{"q"}` | `{"items":[…]}` |

Details:

- `search/repos` — `items` defaults to `[]`. An item with `full_name`
  is a repo; else `org` is an org; items with neither are dropped.
- `org/repos` — `repos` defaults to `[]` (repo names, not full paths).
- `repo/tree` — one entry per path, recursive over the default branch:
  `{"path":"src/main.rs","type":"blob","sha":"…","size":123}` where
  `type` is `"blob"` or `"tree"` (`"tree"` renders as a directory);
  `size` is optional and blobs only. `entries` defaults to `[]`,
  `truncated` to `false`, `branch` to `"main"`.
- `repo/blob` — `bytes_b64` is standard-base64 file content.
- `repo/web_url` — build the browser URL for a repo root (`path` empty),
  a path (tree/blob grammar is the provider's), appending a line
  fragment when `line` is a number (`line` is JSON `null` when absent;
  `branch` may be empty — the provider resolves it).
- `repo/clone_url` — a URL `git clone` accepts.
- `search/code` — `q` is the full query with qualifiers (`repo:`,
  `org:`, `extension:`, `path:`). Items: `{"repo","path","sha","branch",
  "matches":[str,…]}`; `sha` defaults to `""`, `branch` to `"main"`,
  `matches` to `[]`. `matches` are matched substrings — the UI locates
  them in the blob to compute real line numbers and previews.

## The content-id contract

Every `sha` is an opaque **content id**: it MUST change when content
changes. ghx's cache is content-keyed and immutable — trees live at
`~/.cache/ghx/trees/<sha>.json`, blobs at `blobs/<ab>/<rest>`, and are
never invalidated, only evicted. A provider that reuses a sha for
different bytes will show stale content. `fs_provider.py` hashes blob
bytes with sha256 (directories hash their path — they have no content).

## Errors

Reply with a JSON-RPC `error` object instead of `result`:

```
← {"jsonrpc":"2.0","id":3,"error":{"code":1,"message":"no blob abc in local/alpha"}}
```

The `message` becomes the `Err` the UI shows as a one-line status/toast
(`code` is ignored). A reply with neither `result` nor `error` fails
with "provider reply without result". `fs_provider.py` wraps every
handler exception this way.

## Configuration

`~/.config/ghx/config.toml` (or a file passed to `ghx --config`):

```toml
[provider]
kind = "stdio"
command = ["python3", "/path/to/fs_provider.py", "/path/to/code"]
```

`kind = "github"` (the default) uses the built-in provider. An empty
command, a failed spawn, or an unknown kind falls back to GitHub with
a warning on the status line — misconfiguration never blocks startup.

Try the reference adapter against a directory of repos:

```
python3 examples/providers/fs_provider.py ~/code   # serves ~/code/* under "local"
ghx --config provider.toml                          # with the [provider] block above
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

Scaffolding: `skills/ghx-provider/SKILL.md` (in this repo) walks
through building a provider — capability questionnaire, adapter
skeleton, and a conformance test suite that gates integration.
