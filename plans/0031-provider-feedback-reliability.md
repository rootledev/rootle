# 0031 — Provider feedback: observable readiness and scoped failures

Status: **implemented and verified for v0.12.2**.
Priority: **P1 correctness batch**, completed ahead of feature expansion.
Publication requires green CI and website updates.
External-provider changes and platform claims remain outside this release.

## Input and evidence limits

External continued-use report: rootle v0.10.0 → v0.12.1 with
`rootle-bbgithub` pinned to v0.1.2, on WSL Ubuntu 24.04 / glibc 2.39.
The reported tree load contains 311 entries for `tnawara/dotfiles`.
Treat those observations as ground truth, not as results reproduced here.
There is **no RHEL 8.10 evidence**, and no access to the external provider's
implementation is assumed.

Confirmed improvements remain requirements: worker-tracked `settle [ms]`,
structured session logs, working stdio provider composition, and rejection of
`--update --check --headless -` before filesystem effects. Do not regress them.

## Decisions and priorities

| Finding | Decision | Priority / owner |
|---|---|---|
| Thin headless state | Implement explicit, request-scoped tree/search outcomes and counts. Keep existing state keys. | P1, rootle |
| Personal owner inferred to be an org | Stop implicit owner-list RPCs on direct-open and warm startup; separate repository context from organization evidence. | P1, rootle |
| Unrelated owner failure pollutes successful tree status | Guard outcomes before mutation and keep errors with their operation/surface, not a last-writer-wins string. | P1, rootle |
| Search unavailable looks empty | Preserve typed failures and show a durable search-surface error, including failed partial streams. | P1, rootle |
| `protocol: 1` versus specification v1.6 | Document the axes, capability defaults, and limits of handshake evidence. | P1, rootle |
| Provider/rootle package compatibility range and provider `--help` | Request a tested-pair/support statement from the provider maintainer; do not infer a range for them. | P1 external action; not a dependency of rootle UI fixes |
| Machine-readable live provider inspection | Design explicit opt-in inspection separately; keep `provider list --json` passive. | P2, rootle |
| RHEL 8.10 certification | Require real rootle + provider + dependency/auth execution on that target before making claims. | P2, joint deployment/provider validation |
| Negotiated specification-minor or package-version admission | No new negotiation field or version blacklist in this batch. Revisit only with a concrete unmet capability/version requirement. | P3, protocol design gate |
| Always opening search on warm profiles | Not inferred from this report; preserve user recents and current warm-profile policy. | P2, only with explicit UX demand |

No P0 incident is established by the report: browsing works, no provider
fallback is reported, and no data-loss or installation-write regression is
shown. The P1 issues nevertheless give consumers false success/failure signals
and take precedence over feature expansion.

## Source-grounded diagnosis at v0.12.1

### Readiness and status are different facts

- `crates/rootle/src/headless.rs::Headless::settle` checks outstanding workers
  before draining the event queue and follows work spawned by results. Preserve
  that algorithm: quiescence is not provider success or successful rendering.
- `app/presentation.rs::App::snapshot` currently exposes mode, context, overlay
  booleans and free-text status, but no browser tree outcome/counts.
- `components/browser.rs::Browser::diagnostics` and
  `app/diagnostics/state.rs` already expose useful column/preview counts to the
  trace, but lack an authoritative current tree request/outcome. Do not copy
  the entire diagnostic JSON into the public headless contract.
- `AppEvent::TreeLoaded/TreeFailed` carry owner/name, not a revision and domain
  generation. A nonempty cached `Browser::tree` alone cannot prove that the
  current selection/ref/reload succeeded.
- `app/events.rs` applies or clears status for owner/tree outcomes without a
  complete current-operation check in every branch. In particular,
  `OrgReposFailed` writes the global status unconditionally; tree success only
  clears selected loading-text prefixes. The code can retain the reported
  unrelated 404. The exact callback that cleared it in the other reported run
  cannot be identified without that run's full trace.

A `job_finished {job: tree, outcome: ok}` record is a worker outcome, not proof
that its result passed the UI's identity guard. A stale successful result can
be rejected later. The consumer-facing snapshot must describe **accepted state**.

### Repository owners are not organization evidence

- `state.rs::State::record_repo` appends the repository owner to `recent_orgs`.
- `Browser::new` turns every saved `recent_orgs` value into `EntryKind::Org`.
- `app/mod.rs::App::new` starts `LoadOrgRepos` for the initially selected saved
  entry. Both `main.rs::run` and `headless.rs::run_cli` apply an explicit CLI
  repository **after** `App::new`, so a warm-profile owner request can already
  be in flight when direct-open begins.
- `app/actions/browse.rs::RepoSelected` itself requests the tree, not an owner
  listing. A genuinely empty profile and an explicit repository argument need
  not take the same path as a profile reused across runs.
- The in-tree `crates/github/src/client/search.rs::org_repos` directly selects
  `/orgs/{name}/repos`. For stdio, rootle sends `org/repos`; the external
  provider, not rootle's GitHub client, builds the reported HTTP URL.

This is shared responsibility, not merely a provider HTTP bug: rootle must
not request organization operations because a repository has an owner prefix.
The external provider's supported explicit namespace-listing behavior remains
its own contract. Never special-case `tnawara`, drop all 404s, or silently try
unrelated endpoints until one returns data.

### Fresh and warm profiles must not be conflated

`App::build` opens repo search when `recent_repos`, `recent_orgs`, and
`last_repo` are all empty. Otherwise it opens BROWSE. `popup` is the repository
search dialog; `search_view` is the separate global find/grep surface. An empty
typed query and `search_view: false` do not establish a fresh profile.
The report's warm-state path is consistent with this code, but its actual
saved state was not inspected. Do not erase recents to make the symptom vanish.

### Search failures exist, but their surface is insufficient

- `app/workers/search.rs::spawn_view_search` sends typed
  `GlobalSearchFailed {gen_id, error}`. `app/events.rs` checks the view generation
  before forwarding it; it does not put the failure in the modeline.
- `components/global_search/results.rs::update` converts `ProviderError` to an
  `Option<String>` and clears all hits on failure, including streamed hits.
- `components/global_search/render/results.rs::render_results` does have an
  error-title branch. It is misleading to claim there is no rendering code at
  all. However, the body remains empty, the title is a width-limited muted
  border, and an expanded preview returns before that error-title branch.
  The report establishes that the failure was not usefully visible; it does
  not establish which layout/overlay detail hid it on that laptop.
- Stream progress sets status to `searching <forge>… N hits`, while final paths
  clear only `searching code…`. Remove this text-prefix ownership dependency
  from the affected search lifecycle.

## M1 — Typed operation outcomes and public headless state (P1)

Introduce small domain-owned request/outcome types, not a generic job framework.

- Add typed tree and owner-list request identities in `request.rs`, following
  `HistoryRequest`: repository/owner identity, requested `GitRef` where relevant,
  and a domain-tagged generation. Invalidate on selection/ref/reload/provider
  replacement. Owner listing is now explicit-only: remove ambient startup
  warming rather than adding a purpose enum for work that no longer exists.
  Do not derive identity from pane captions.
- Update worker events and their success/failure handlers together. Reject
  obsolete outcomes **before** touching data, focus, status, or load state.
  Apply this to injected action paths as well as worker-event paths.
- Store `idle | loading | ready | failed` with the current resource request.
  A successful empty result is `ready` with count zero, never an inferred
  failure. A failed reload must not advertise a retained old tree as a fresh
  success. If old content remains visible, expose its separate identity and
  stale/retained status rather than merging it into current-result counts.
- Keep `ProviderError` typed internally. Project bounded, sanitized
  `{kind, message, retry_after_s}` at the observation boundary. Do not reconstruct
  an error kind by parsing its formatted message.
- Use the same state model for terminal and headless operation. Snapshots and
  diagnostics may share typed observations, but retain their different privacy
  policies: metadata traces still omit sensitive text unless explicitly enabled.

### Additive state contract

Add `state_schema_version: 1` while retaining existing top-level fields,
including `status`, `popup`, `search_view`, and the commit `surface` object.
`status` remains presentation text, not an automation predicate.

Target shape below is an **illustrative proposed excerpt**, not output already
produced by v0.12.1:

```json
{
  "state_schema_version": 1,
  "browser": {
    "tree": {
      "phase": "ready",
      "request": {
        "repository": "tnawara/dotfiles",
        "revision": null,
        "generation": 1
      },
      "entry_count": 311,
      "truncated": false,
      "branch": "main",
      "error": null
    }
  },
  "search": null
}
```

Define each field, not just an example:

- `browser.tree.entry_count`: number of entries in the accepted recursive tree,
  including directory entries supplied by the provider; independent of local
  filtering, pane focus, and terminal dimensions. Null before a current result
  exists or after current-request failure; zero is valid success.
- `truncated` and `branch`: accepted provider metadata, null when unknown.
  `request.revision: null` means the default revision was requested; do not
  invent a resolved commit SHA that the provider did not return.
- `browser.pane`: current repository-relative path, unfiltered immediate-child
  count, and post-filter visible count. These are explicitly different from
  recursive tree entry count.
- `browser.owner_list`: its own request/outcome/error, so a user can inspect an
  owner-list failure without treating the repository tree as failed.
- `search`: null when closed; otherwise kind, submitted request identity,
  phase, retained result count, visible result count, known truncation state,
  and typed error. Keep edited input distinct from the submitted query whose
  results are displayed. Capture the submitted kind/scope/query/extension for
  result styling; a later input edit must not relabel in-flight results.
- Never serialize complete entries, source buffers, raw RPC bodies, credentials
  or environment vectors merely to expose readiness. Keep the shape bounded.

Do not equate `pending_workers == 0` with ready data: a final event may still
need applying. Keep `settle` as the wait and make `state` a non-waiting observation
of accepted model state. `ready` does not certify that pixels were painted or
that no overlay covers the browser; use `frame` when actual cells are the target.

Document a CI recipe using `settle` followed by `state`, then checking the
requested repository/ref, tree phase, count and truncation policy. Do not use
`status == null`, a fixed expected count of 311, or a `job_finished` record as
universal success criteria. A repo-specific test may of course assert its own
known entries/counts.

Likely seams: `request.rs`, `event.rs`, `app/workers/lifecycle.rs`,
`app/events.rs`, `app/actions/browse.rs`, `components/browser*`,
`app/presentation.rs`, and `app/diagnostics/state.rs`. Keep snapshot projection
in a cohesive sibling module rather than growing the event dispatcher.

## M2 — Intent-driven startup, owner semantics and scoped status (P1)

- Resolve launch intent before background browsing work begins in both drivers.
  Direct `owner/repo[@ref]` open must request that tree, not an unrelated saved
  owner listing. Keep provider composition/consent behavior intact.
- Remove unconditional startup owner-list warmup. A warm profile may still open
  BROWSE with its recents, but does not automatically probe all/first saved
  namespaces. A fresh profile still opens the repo-search `popup`.
- Stop promoting repository owners into confirmed organization history.
  Preserve genuine repository recents and explicit organization selections.
  Legacy `recent_orgs` entries have mixed provenance: retain them as unclassified
  history, not confirmed organizations, and do not probe them automatically.
  Do not infer owner kind from slash syntax, display names, or provider name.
- Record enough selection provenance to distinguish explicit organization
  browsing from repository context. Do not offer an organization search scope
  merely because a personal/unclassified repository owner exists. Provider
  replacement must invalidate any provider-specific classification evidence.
- In-tree GitHub: on an explicit owner-list operation, resolve owner metadata
  inside `rootle-github` and select `/users/{login}/repos` for a personal account
  or `/orgs/{login}/repos` for an organization. Cache only valid classification;
  preserve auth/rate-limit/network/not-found failures instead of converting any
  404 into a successful fallback or guessing identity from text.
- Stdio: do not require a new mandatory method/capability for direct repo open.
  `org/repos` retains its legacy wire spelling. Rootle must not invoke it just
  because it derived an owner. Explicit personal-owner enumeration beyond an
  adapter's contract requires adapter support; any refusal stays an explicit,
  operation-scoped error, not a fake empty listing.
- An owner result must not steal focus, rebuild a newer tree's columns, clear
  another operation's loading state, or replace the tree's status. Foreground
  errors remain visible on their own surface. Ancillary outcomes remain available
  in structured state/diagnostics without contaminating successful foreground
  work. Replace prefix-based status clearing in these paths with ownership.

## M3 — Durable search failures and partial-result honesty (P1)

- Replace `GlobalSearch.error: Option<String>` as the authoritative failure state
  with typed current-request outcome/error data. Keep the existing generation
  protection and apply it consistently to deltas, final outcomes, closure,
  resubmission and provider replacement.
- Show a bounded, readable error region in the results surface: failure headline,
  error kind, provider message and applicable guidance. It must not live only in
  the border title or transient modeline. Wrap/fit through shared rendering
  primitives; important information remains reachable on narrow terminals.
- An auth error remains `auth`, even if the provider's message describes a missing
  Spaces-only tool. Do not hardcode grok, infer rootle/provider incompatibility,
  recategorize from prose, or trigger provider fallback. `degraded` remains about
  provider composition, not an individual unavailable operation.
- Distinguish successful zero matches, unsupported capability, loading, and failed
  search. Capability-false paths explain unavailability without issuing a request.
- A final failure after streamed results retains that request's already accepted
  hits and marks them as incomplete/partial, with the failure visible. Do not
  clear useful data or imply complete results. An expanded hit must not conceal
  the parent search failure; preserve access to the notice from that surface.
- New submission clears the previous terminal failure and starts a new request.
  Retry through the existing submit path; no automatic retries/backoff machinery.
  Cancel/supersede prevents late failures or results from affecting the new view.

Likely seams: `components/global_search.rs`, `global_search/results.rs`,
`global_search/render/results.rs`, `app/actions/search.rs`, `app/events.rs`,
`app/workers/search.rs`, and the shared observation projection from M1.

## M4 — Compatibility documentation, without false negotiation (P1)

Document these distinct axes in contributor protocol docs, public provider docs,
and the provider-scaffolding guidance:

1. rootle and provider package versions are independently released artifacts.
2. `jsonrpc: "2.0"` identifies the RPC envelope, not rootle's feature level.
3. `initialize.protocol: 1` identifies the application wire-contract major.
4. Specification **v1.6** names the cumulative additive extensions within that
   major. It is not a negotiated `1.6` field and does not mean every optional
   operation exists. Capabilities and method/field defaults govern those features.

Pin exact artifacts and consult the provider's tested pairs, required methods,
capabilities and runtime requirements. A successful handshake is not a guarantee
that optional grep, credentials, external tools, OS dependencies or every rootle
feature will work. One observed working pair does not establish an open-ended
minimum/maximum rootle version range.

The capability-default table must match `handshake.rs`: core org/code-search
flags default true, file-search inherits code-search when absent, and
refs/log/blame/commit default false. Remove the contradictory shorthand that
all capabilities default enabled; repair the duplicated/fenced error-kind text
while updating the protocol document.

Current implementation caveat: a missing or non-u64 handshake `protocol` value
is normalized to 1 by `unwrap_or(1)`. The lifecycle log therefore records the
client's effective accepted value, not necessarily an explicit provider claim.
Describe that tolerance separately from the normative provider requirement to
send integer 1. Do not tighten admission or silently require v1.6 in this batch.
Track declared-versus-effective diagnostics and admission policy under P2 below.

## Verification and implementation ordering

M1 supplies the request/outcome foundation. M2 and M3 can then proceed with
separate ownership of browser/startup and search components; one integration
owner handles shared event/snapshot mutation. M4 is independently actionable.

Required regression matrix:

- Fresh HOME/state: repo-search popup, no seeded owners and no owner-list RPC.
- Warm legacy state: personal owner retained, no eager owner RPC; no forced wipe
  or newly mandatory launch popup. Explicit repo argument wins over warm state.
- Direct personal repo, including nested provider repo IDs: tree accepted and
  ready; no rootle-invented organization classification/request.
- Explicit organization/personal listing in the built-in provider uses the right
  endpoint. Auth/rate-limit/network/not-found failures remain real failures.
- Owner failure before and after tree success yields the same successful current
  tree state. Late owner success cannot truncate/rebuild the current tree pane.
- Empty successful tree is ready/count 0; failed tree is failed, not ready/count 0.
  Ref changes, reloads, repository switches and provider replacement reject stale
  successes and failures. Retained prior data never masquerades as a fresh result.
- Search capability false, auth/provider/unknown-kind failures, successful empty
  search, and rate-limit retry-after data are visibly and structurally distinct.
- Streamed hits followed by failure remain visible and explicitly partial; an
  expanded hit does not hide the failure. New query/retry/cancellation/supersession
  removes only the appropriate state. Edited-but-unsubmitted input does not change
  the submitted request's identity or presentation.
- Cover real headless scripts and actual PTY frames at the reviewer's 140×45
  viewport and a narrow viewport, with control characters and long messages.
  Assert state and visible errors, not exact error wording or timing heuristics.
- A loopback stdio fixture supports deterministic delayed/reordered responses,
  not a real-account dependency. Keep actual-provider confirmation separate and
  opt-in; do not require grok/Spaces or mutate the provider installation in CI.
- Preserve `settle` timeout/follow-up behavior and the read-only-install CLI
  conflict regression. `--headless` must never enter any update route.

Run the Docker test and e2e gates after integration; run provider conformance and
model gates if the wire/transport contract is touched. Capture before/after state
and frames; verify trace completion before using logs as evidence. Publish only
after this matrix passes, with release notes distinguishing fixes from deferred work.

Local evidence: the Docker fmt/clippy/workspace-test gate and **79 e2e tests**
pass; canonical provider conformance passes **47 cases**, and the bounded model
gate remains green. New scenarios exercise 140×45 and 40×10 PTYs, reordered
real-child tree replies, retained partial search failures, frozen-query styling,
and metadata/full trace privacy. Private before/after frames compare the
checksum-verified v0.12.1 binary with the current build. This is controlled
fixture evidence, not additional bbgithub or RHEL coverage.

## Deferred work and explicit ownership

- **P1 external — rootle-bbgithub maintainer:** publish tested rootle/provider
  version pairs, required wire major/capabilities, useful `--help` or equivalent
  install documentation, and laptop versus Spaces runtime dependencies. Document
  personal-owner enumeration support. Rootle cannot truthfully invent this matrix
  or commit changes to an uninspected external provider.
- **P2 rootle — opt-in provider inspection:** expose actual negotiated/observed
  protocol and capabilities, including declared versus effective/defaulted values.
  Live probing must be explicit because it executes provider code; plain
  `provider list --json` remains receipt-only and must not spawn, authenticate,
  install, upgrade, or silently turn stale cached observations into guarantees.
  Decide malformed/missing-protocol admission with conformance and compatibility
  evidence rather than silently changing legacy tolerance in M4.
- **P2 joint deployment validation — RHEL 8.10:** exercise rootle, the pinned
  provider binary, loader/library requirements, credentials and optional search
  tools on that target. Static rootle alone cannot certify a child provider.
  Until then, report only the supplied WSL Ubuntu/glibc coverage.
- **P2 conditional UX — always-search startup for warm profiles:** separate opt-in
  behavior only if requested; not a fix for mixed owner provenance.
- **P3 protocol gate — minor-version/range negotiation:** revisit only when major 1
  plus explicit capabilities/defaults cannot describe a demonstrated requirement.
  No speculative min/max admission, compatibility blacklist, new wire major,
  automatic provider fallback, or ABI promise in this work.
