---
name: ghx-provider
description: Scaffold a ghx provider — an adapter that wraps any source-control backend (internal or public) behind ghx's stdio NDJSON-RPC protocol so the TUI can browse it. Grills the implementer on their backend first, then generates the adapter skeleton, config, and a conformance + e2e test suite that MUST pass before the provider integrates. Use when someone wants ghx to talk to a new backend.
---

# ghx provider scaffolding

A provider is a child process ghx spawns and talks to over
newline-delimited JSON-RPC 2.0 on stdio (the LSP model). The TUI knows
nothing about backends — it only speaks the protocol.

Ground truth (read before scaffolding, cite them to the user):

- Protocol spec: `doc/provider-protocol.md`
- Reference implementation: `examples/providers/fs_provider.py`
- Rust side (what ghx parses): `src/provider/stdio.rs`
- In-tree alternative (Rust only): `src/provider/mod.rs` (`trait Provider`)

## 1. Grill the implementer — STOP until every block is answered

Do not scaffold on vague answers. Go through every block; for each,
record the answer — the scaffold and the tests encode them.

**Backend shape**
1. What is the backend? (HTTP API, CLI tool, database, git server, …)
2. Language/runtime for the adapter, and why (must run wherever ghx
   runs; stdlib-only is a virtue).
3. Can it list repos? Under what grouping (org/team/project/none)?
   Map to `search/repos` + `org/repos`.

**Identity & content ids (the contract that breaks caches if wrong)**
4. Repo id scheme: ghx treats repos as opaque `"group/project"`
   strings. What is yours? (Must contain exactly one `/`.)
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
11. Binary files: serve raw bytes; ghx detects binary and renders a
    placeholder. No base64 of huge files.

**URLs (yank + clone go through the provider — no assumptions)**
12. Web URL grammar for: repo root, file at branch+line, directory,
    org page. Concrete examples.
13. Clone URL the local `git` accepts (https/ssh/…)? If cloning is
    unsupported, say so — `clone_url` returns an error, the wizard
    reports it.

**Auth & operations**
14. Where does the adapter get credentials? (Env, config file,
    keychain — ghx NEVER sees provider auth.) Never log them.
15. Rate limits/cost per call? Where does the adapter cache (it owns
    its own caching; ghx caches nothing for stdio providers)?
16. Errors: map backend failures to short human messages — they land
    verbatim in the TUI status line.
17. Multiplexing: is this one backend or a fan-out over several (e.g.
    tool A for search, tool B for repos)? ghx supports exactly one
    provider process; fan-out lives inside the adapter.

If any answer is "unknown", scaffold with that capability DISABLED
(capability flag false / method returns an error) and leave a TODO —
never fake data. The UI must stay honest.

## 2. Scaffold

Create a new directory (suggest `providers/<name>/` or a separate
repo) with:

```
<name>/
  provider.py          # the adapter skeleton (below)
  provider.toml        # ghx config pointing at it
  test_conformance.py  # protocol contract tests — the gate
  test_e2e.py          # drives the real ghx binary against it
  README.md            # the grill answers, one line each
```

### provider.py — red skeleton

Every method returns a JSON-RPC error until implemented; the
conformance suite fails red and goes green method by method.

```python
#!/usr/bin/env python3
"""<name> — ghx stdio provider wrapping <backend>.

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


def search_code(params):  # -> {"items": [{"repo","path","sha","branch","matches":[..]}]}
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

Run: `ghx --config provider.toml`.

### test_conformance.py — the integration gate

Self-contained; only needs python3 + pytest. It speaks the protocol
directly (no TUI) and encodes the EXACT contract ghx's serde parses —
field names, types, defaults, and the content-id rule. All of it must
pass before wiring the provider into anyone's config.

```python
"""ghx provider conformance — every test must pass before integration.

Run:  PROVIDER="python3 provider.py <fixture-root>" python3 -m pytest \
      test_conformance.py -v          # stdlib + pytest only
"""
import base64, json, os, shlex, subprocess

import pytest

CMD = shlex.split(os.environ["PROVIDER"]) if os.environ.get("PROVIDER") else [
    "python3", "provider.py"
]


class Rpc:
    def __init__(self):
        self.p = subprocess.Popen(CMD, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True)
        self.i = 0

    def call(self, method, params):
        self.i += 1
        self.p.stdin.write(json.dumps(
            {"jsonrpc": "2.0", "id": self.i, "method": method,
             "params": params}) + "\n")
        self.p.stdin.flush()
        for _ in range(100):
            line = self.p.stdout.readline()
            assert line, "provider closed stdout"
            msg = json.loads(line)
            if msg.get("id") == self.i:
                if "error" in msg:
                    raise AssertionError(f"{method}: {msg['error']}")
                return msg["result"]

    def close(self):
        self.p.terminate()
        self.p.wait(timeout=5)


@pytest.fixture(scope="module")
def rpc():
    r = Rpc()
    yield r
    r.close()


def test_initialize(rpc):
    r = rpc.call("initialize", {"protocol": 1})
    assert r["protocol"] == 1
    assert isinstance(r["name"], str) and r["name"]
    caps = r["capabilities"]
    assert isinstance(caps.get("orgs"), bool)
    assert isinstance(caps.get("code_search"), bool)


def test_search_repos_shape(rpc):
    # Skip honestly if the capability is off: error, not fake data.
    try:
        r = rpc.call("search/repos", {"query": ""})
    except AssertionError as e:
        pytest.skip(f"no repo search: {e}")
    items = r["items"]
    assert isinstance(items, list)
    for it in items[:5]:
        assert ("full_name" in it) ^ ("org" in it), "one of full_name/org"
        if "full_name" in it:
            assert it["full_name"].count("/") == 1


def test_tree_shape_and_blob_roundtrip(rpc):
    # The scaffold's fixture must contain a known repo with a known
    # file — adjust REPO/FILE/TEXT for your fixture.
    REPO, FILE, TEXT = "team/proj", "hello.txt", b"hello ghx\n"
    r = rpc.call("repo/tree", {"repo": REPO})
    assert isinstance(r["entries"], list) and r["entries"]
    for e in r["entries"]:
        assert set(e) >= {"path", "type", "sha"}
        assert e["type"] in ("blob", "tree")
    entry = next(e for e in r["entries"] if e["path"] == FILE)
    blob = rpc.call("repo/blob", {"repo": REPO, "sha": entry["sha"]})
    assert base64.b64decode(blob["bytes_b64"]) == TEXT


def test_content_id_contract(rpc):
    """sha changes iff content changes — the cache's foundation."""
    # Requires a fixture hook: CHANGE_CMD mutates the known file.
    cmd = os.environ.get("CHANGE_CMD")
    if not cmd:
        pytest.skip("set CHANGE_CMD to test the content-id contract")
    REPO, FILE = "team/proj", "hello.txt"
    before = rpc.call("repo/tree", {"repo": REPO})
    sha1 = next(e["sha"] for e in before["entries"] if e["path"] == FILE)
    subprocess.run(shlex.split(cmd), check=True)
    after = rpc.call("repo/tree", {"repo": REPO})
    sha2 = next(e["sha"] for e in after["entries"] if e["path"] == FILE)
    assert sha1 != sha2, "content changed but sha did not"


def test_urls_shape(rpc):
    for method, params, field in [
        ("repo/web_url", {"repo": "team/proj", "path": "", "branch": "", "line": None}, "url"),
        ("org/url", {"org": "team"}, "url"),
        ("repo/clone_url", {"repo": "team/proj"}, "clone_url"),
    ]:
        try:
            r = rpc.call(method, params)
        except AssertionError as e:
            pytest.skip(f"{method} unsupported: {e}")
        # web/org URLs need a scheme; clone targets may also be
        # absolute local paths (git accepts them — fs provider does).
        assert isinstance(r[field], str) and (
            r[field].startswith(("http://", "https://", "ssh://", "file://", "git@"))
            or (field == "clone_url" and r[field].startswith("/"))
        )


def test_error_shape(rpc):
    rpc.p.stdin.write(json.dumps(
        {"jsonrpc": "2.0", "id": 999, "method": "repo/tree",
         "params": {"repo": "no/such"}}) + "\n")
    rpc.p.stdin.flush()
    line = rpc.p.stdout.readline()
    msg = json.loads(line)
    assert msg["id"] == 999
    assert "error" in msg and "message" in msg["error"]
    assert "result" not in msg
    rpc.i = 999  # resync the id counter
```

Field-name gotchas that break ghx's serde silently (defaults absorb
wrong names — the TUI shows empty data, not an error):

- tree entry: `type` (not `kind`), values exactly `"blob"`/`"tree"`
- blob: `bytes_b64` (raw base64, no headers/whitespace)
- search/code: `matches` is a list of STRING match texts
- optional fields ghx defaults: `truncated:false`, `branch:"main"`,
  `sha:""`, `matches:[]`, `items:[]`, `repos:[]`

### test_e2e.py — the real TUI against the provider

Only after conformance is green. Copy the pattern from
`e2e/test_provider.py` in the ghx repo (PTY harness, `--config`), or
minimum bar without the harness:

```bash
ghx --config provider.toml   # manual: search → open repo → preview
```

Required e2e assertions (with the ghx repo's e2e harness):

1. search finds the fixture repo; Enter opens it; tree pane lists
   `hello.txt`
2. preview shows the blob content (repo/blob works end to end)
3. provider process dies when ghx exits (lifecycle)

## 3. Definition of done

- [ ] every grill answer recorded in `<name>/README.md`
- [ ] `test_conformance.py` fully green (skips only for honestly
      disabled capabilities)
- [ ] content-id contract test green (`CHANGE_CMD` provided)
- [ ] e2e against the real binary green
- [ ] `provider.toml` committed; README shows the run command
- [ ] credentials documented by NAME only (where they live), never
      values, never logged
