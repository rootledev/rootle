# 0005 — Provider seam: rootle talks to backends through a trait, not GitHub

The TUI must not depend on GitHub: internal source-control systems
can't be reached through the GitHub REST API. The app speaks to
backends through `trait Provider` (`src/provider/mod.rs`); GitHub is
the in-tree reference implementation, and external providers are child
processes speaking NDJSON-RPC 2.0 over stdio (the LSP model).

## 1. Decisions

- **Seam first, GitHub wiring against the trait** — no throwaway
  internal-only integration. GitHub ships as the default in-tree
  provider; others (GitLab, internal) live in separate repos.
- **Stdio transport over local HTTP** — zero config (no ports/auth on
  localhost), rootle owns the lifecycle (spawn at start, dies with the
  app; verified by e2e). JSON-RPC 2.0 newline-delimited framing; the
  same envelope could ride HTTP later without protocol changes.
- **One active provider at a time** — multiplexing across systems is
  the provider's own job (a wrapper script can fan out to tool 1 for
  code search, tool 2 for repos).
- **No push notifications in v1** — cache freshness stays
  revalidation-based inside each provider.

## 2. The seam (src/provider/mod.rs)

```rust
trait Provider {
    fn name(&self) -> &str;
    fn capabilities(&self) -> Capabilities;   // { orgs, code_search }
    fn default_orgs(&self) -> Vec<String>;    // cold-start suggestions
    fn search(&self, query) -> Result<Vec<SearchItem>>;
    fn org_repos(&self, org) -> Result<Vec<String>>;
    fn fetch_tree(&self, repo) -> Result<TreeResult>;   // entries + branch
    fn fetch_blob(&self, repo, sha) -> Result<Vec<u8>>;
    fn search_code(&self, q) -> Result<Vec<CodeMatch>>;
    fn clone_url(&self, repo) -> Result<String>;
    fn web_url(&self, repo, path, branch, line) -> Result<String>;
    fn org_url(&self, org) -> Result<String>;
}
```

Contract rules (spec: `doc/provider-protocol.md`):

- Repos are opaque `"group/project"` strings — the UI never parses them.
- `sha` is an opaque **content id**: it MUST change when content
  changes (the GitHub provider's disk cache is content-keyed and
  immutable; that cache lives *inside* the provider, invisible to the
  TUI).
- URL building (yank) and clone URLs come from the provider — no
  GitHub URL grammar outside `src/provider/github.rs`.
- Capability flags drive UI degradation; anonymous GitHub = code
  search reports "needs a token" as a normal error toast.
- Provider misconfiguration never blocks startup: `build()` falls back
  to GitHub with a status-line warning.

## 3. Layout after the seam

```
src/provider/
  mod.rs      — trait, shared types, build(), offline() (tests)
  github.rs   — GitHubProvider wrapping src/github/ (client + cache)
  stdio.rs    — StdioProvider: spawn, handshake, mutex-serialized RPC
src/github/
  client.rs   — REST client (auth chain, ETag revalidation)
  cache.rs    — content-addressable disk store (provider-internal)
  types.rs    — wire models only (serde)
examples/providers/fs_provider.py  — reference adapter (local dirs as
                                     repos, content-hash shas, grep)
```

## 4. Verification

- e2e (`e2e/test_provider.py`): the whole app on the fs stdio provider,
  offline — repo search → tree → blob preview → global grep with
  folded regions, plus child-process lifecycle (dies with the app).
- `e2e/test_global_search.py` and `e2e/test_wiring.py` run the v0.2
  search view and yank/settings/clone over the same provider.
- Live GitHub code search verified manually with a real token (repo
  scope, folded 105-match file, real line numbers).
