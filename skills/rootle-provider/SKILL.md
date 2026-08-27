---
name: rootle-provider
description: Scaffold a rootle provider — an adapter that wraps any source-control backend (internal or public) behind rootle's stdio NDJSON-RPC protocol so the TUI can browse it. Grills the implementer on their backend first, then generates the adapter skeleton and config; the canonical forge-conformance suite (rootledev/forge-conformance) is THE gate that MUST pass before the provider integrates. Use when someone wants rootle to talk to a new backend.
---

# rootle provider scaffolding

A provider is a child process rootle spawns and talks to over
newline-delimited JSON-RPC 2.0 on stdio (the LSP model). The TUI knows
nothing about backends — it only speaks the protocol.

Ground truth (read before scaffolding, cite them to the user):

- Protocol spec: `doc/provider-protocol.md`
- Reference implementation: `examples/providers/fs_provider.py`
- Rust side (what rootle parses): `src/provider/stdio.rs`
- In-tree alternative (Rust only): `src/provider/mod.rs` (`trait Provider`)

## 1. Grill the implementer — STOP until every block is answered

Do not scaffold on vague answers. Go through every block; for each,
record the answer — the scaffold and the tests encode them.

**Backend shape**
1. What is the backend? (HTTP API, CLI tool, database, git server, …)
2. Language/runtime for the adapter, and why (must run wherever rootle
   runs; stdlib-only is a virtue).
3. Can it list repos? Under what grouping (org/team/project/none)?
   Map to `search/repos` + `org/repos`.

**Identity & content ids (the contract that breaks caches if wrong)**
4. Repo id scheme: rootle treats repos as opaque `"group/project"`
   strings. What is yours? (One or more `/` — nested groups like
   `group/sub/project` are legal; every rootle parse site splits on
   the FIRST slash, org = first component.)
5. **Content ids (`sha`)**: can you hash file content (sha256 of
   bytes)? REQUIRED: the id MUST change when content changes and MUST
   NOT change when it doesn't. If your backend has no such id, the
   adapter must compute one. There is no way around this.
6. Branches/ref model: is there a "default branch" concept? Does a
   tree listing need a ref, or is it "current state"?

**Capabilities (be honest — the UI degrades gracefully)**
7. Code search: can you match file *contents*? Semantics (substring,
   case-insensitive?) — and can you report the matched substrings?
8. Orgs: can you list an org's repos, or only search?
9. Trees: full recursive listing? Size caps — declare `truncated: true`
   past them (the UI shows it).

**Blobs**
10. Size limits on serving file bytes? (Suggest 1 MiB like GitHub;
    larger files are preview-rejected anyway.)
11. Binary files: serve raw bytes; rootle detects binary and renders a
    placeholder. No base64 of huge files.

**URLs (yank + clone go through the provider — no assumptions)**
12. Web URL grammar for: repo root, file at branch+line, directory,
    org page. Concrete examples.
13. Clone URL the local `git` accepts (https/ssh/…)? If cloning is
    unsupported, say so — `clone_url` returns an error, the wizard
    reports it.

**Auth & operations**
14. Where does the adapter get credentials? (Env, config file,
    keychain — rootle NEVER sees provider auth.) Never log them.
15. Rate limits/cost per call? Where does the adapter cache (it owns
    its own caching; rootle caches nothing for stdio providers)? If you
    cache on disk, use `~/.cache/rootle/providers/<name>/` — never the
    TUI's root. The GitHub provider's layout
    (`~/.cache/rootle/providers/github/`) is the reference: sha-keyed
    immutable blobs/trees, ETag-revalidated ref mappings, atomic
    writes, LRU eviction + orphan sweep. Copy that shape.
16. Errors: map backend failures to short human messages — they land
    verbatim in the TUI status line.
17. Multiplexing: is this one backend or a fan-out over several (e.g.
    tool A for search, tool B for repos)? rootle supports exactly one
    provider process; fan-out lives inside the adapter.
18. Staleness (v1.1): can you tell when a `search/code` hit's placement
    is unverified (index older than the blob it claims)? If yes, emit
    `located: false` on that item — rootle shows a `stale` chip until
    client-side locating heals it. Default (absent) means verified.
19. Error kinds (v1.1): map backend failures to `error.data.kind` —
    `auth`, `rate_limited` (with optional `retry_after_s`),
    `not_found`, `network`, `timeout`, `provider`. Open enum; unknown
    kinds degrade to the message toast. Optional but cheap.
20. Cancellation (v1.1): does a call burn quota or run long? rootle
    sends advisory `$/cancelRequest` notifications for superseded work.
    Ignoring them is legal (they are notifications — never reply); a
    provider that can abort SHOULD.

If any answer is "unknown", scaffold with that capability DISABLED
(capability flag false / method returns an error) and leave a TODO —
never fake data. The UI must stay honest.

## 2. Scaffold

Create a new directory (suggest `providers/<name>/` or a separate
repo) with:

```
<name>/
  provider.py          # the adapter skeleton (below)
  provider.toml        # rootle config pointing at it
  test_e2e.py          # drives the real rootle binary against it
  README.md            # the grill answers, one line each
```

No `test_conformance.py` is generated — conformance comes from the
canonical suite (below), never a copy.

### provider.py — red skeleton

Every method returns a JSON-RPC error until implemented; the
conformance suite fails red and goes green method by method.

```python
#!/usr/bin/env python3
"""<name> — rootle stdio provider wrapping <backend>.

Answers (from the grill — keep updated):
  repo ids:      <e.g. "team/project">
  content ids:   <sha256 of blob bytes>
  capabilities:  orgs=<yes/no> code_search=<yes/no>
  auth:          <env/config/keychain — never printed>
"""
import base64, hashlib, json, sys

PROTOCOL = 1
TODO = NotImplementedError  # raises -> JSON-RPC error, suite stays red


def initialize(params):
    return {
        "protocol": PROTOCOL,
        "name": "<name>",
        "capabilities": {"orgs": False, "code_search": False},
    }


def search_repos(params):  # -> {"items": [{"full_name": "g/p"} | {"org": "g"}]}
    raise TODO


def org_repos(params):  # -> {"repos": ["project", ...]}
    raise TODO


def repo_tree(params):  # -> {"entries": [{"path","type","sha","size"?}], "truncated", "branch"}
    raise TODO


def repo_blob(params):  # -> {"bytes_b64": ...}
    raise TODO


def search_code(params):  # -> {"items": [{"repo","path","sha","branch","matches":[..],"located"?}]}
    raise TODO


def repo_web_url(params):  # -> {"url": ...}  (repo, path, branch, line)
    raise TODO


def org_url(params):  # -> {"url": ...}
    raise TODO


def repo_clone_url(params):  # -> {"clone_url": ...}
    raise TODO


METHODS = {
    "initialize": initialize,
    "search/repos": search_repos,
    "org/repos": org_repos,
    "repo/tree": repo_tree,
    "repo/blob": repo_blob,
    "search/code": search_code,
    "repo/web_url": repo_web_url,
    "org/url": org_url,
    "repo/clone_url": repo_clone_url,
}


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(req.get("id"), int):
            continue  # notification (e.g. $/cancelRequest) — never reply
        try:
            reply = {"jsonrpc": "2.0", "id": req.get("id"),
                     "result": METHODS[req["method"]](req.get("params") or {})}
        except Exception as e:  # noqa: BLE001 — surfaced in the TUI
            reply = {"jsonrpc": "2.0", "id": req.get("id"),
                     "error": {"code": 1, "message": str(e)}}
        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
```

### provider.toml

```toml
[provider]
kind = "stdio"
command = ["python3", "/abs/path/to/<name>/provider.py", "<backend args>"]
```

Run: `rootle --config provider.toml`.

### conformance — the canonical gate

The integration gate is the canonical suite,
[rootledev/forge-conformance](https://github.com/rootledev/forge-conformance):
every protocol gotcha as a numbered case (FC-001..080 — handshake,
content ids, trees/blobs, search, streaming, lifecycle, errors,
`limit`, icons), a deterministic fixture, and per-case spec citations
in failure output. Run it against your adapter:

```bash
git clone https://github.com/rootledev/forge-conformance
cd forge-conformance
python3 run -- python3 /abs/path/to/<name>/provider.py
```

The runner appends the canonical fixture dir as the adapter's last
argv (or substitutes `{fixture}`) and scrubs credential env vars —
the fixture must be servable with no credentials (they are lazy, per
the protocol's restart obligations). **Every case must pass before
the provider integrates.** Do not copy cases into the adapter repo —
copies drift; that is exactly what the canonical suite replaced.


Field-name gotchas that break rootle's serde silently (defaults absorb
wrong names — the TUI shows empty data, not an error):

- tree entry: `type` (not `kind`), values exactly `"blob"`/`"tree"`
- blob: `bytes_b64` (raw base64, no headers/whitespace)
- search/code: `matches` is a list of STRING match texts
- optional fields rootle defaults: `truncated:false`, `branch:"main"`,
  `sha:""`, `matches:[]`, `items:[]`, `repos:[]`, `located:true`
- notifications (no numeric top-level `id`) get NO reply — replying
  with `"id": null` confuses rootle's id matching

### test_e2e.py — the real TUI against the provider

Only after conformance is green. Copy the pattern from
`e2e/test_provider.py` in the rootle repo (PTY harness, `--config`), or
minimum bar without the harness:

```bash
rootle --config provider.toml   # manual: search → open repo → preview
```

Required e2e assertions (with the rootle repo's e2e harness):

1. search finds the fixture repo; Enter opens it; tree pane lists
   `hello.txt`
2. preview shows the blob content (repo/blob works end to end)
3. provider process dies when rootle exits (lifecycle)

## 3. Definition of done

- [ ] every grill answer recorded in `<name>/README.md`
- [ ] forge-conformance fully green against the adapter (all FC cases;
      the suite skips only what honestly-disabled capabilities gate)
- [ ] e2e against the real binary green
- [ ] `provider.toml` committed; README shows the run command
- [ ] credentials documented by NAME only (where they live), never
      values, never logged
