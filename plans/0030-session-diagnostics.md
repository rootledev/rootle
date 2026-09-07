# 0030 — Diagnostic session traces

Status: **implemented and verified (2026-09-07), shipping in v0.11.0**.
Private bounded diagnostics cover both drivers and the provider/CLI paths.
This is recording, not deterministic replay of remote services.

## Problem and reference

Strop's `plans/0029-session-tracing.md`, `crates/strop-trace/`, and
`crates/strop/src/editor/trace/` establish the useful contract: one private,
bounded JSONL file; lazy producers; a dedicated writer; explicit failure and
completion; common input/state/render instrumentation; correlated service
traffic. Its later forensic editor replay is a separate concern.

Rootle currently checks `ROOTLE_TRACE` and reopens a plain-text append file
for each record. Callers eagerly format even when disabled, failures vanish,
and coverage omits most input, state transitions, rendering, transport,
cache, recovery and failure decisions. A trace cannot explain a stale result
or establish whether it ended normally.

## User controls

- `rootle --log` / `--log=ALL`: all diagnostic categories, a newly created
  session file under the XDG state directory's `rootle/logs/`.
- `--log=PATH` or `--log-file PATH`: select a new file. Paths stay native
  `PathBuf`/`OsString`; the two explicit controls conflict.
- Existing `ROOTLE_TRACE=PATH` remains the environment control. Explicit CLI
  selection wins. Its output becomes structured JSONL, not a second sink.
- `--log-content` explicitly enables sensitive input/UI content and provider
  stderr capture. It requires a selected trace (CLI or environment).
- Controls work for interactive, headless, provider-manager and update flows.
  `--help`/`--version` keep clap's side-effect-free early exit.
- Existing files and symlinks are refused, never appended to or truncated.
  Explicit missing parent directories are errors. Automatic log directories
  are private where newly created. A requested trace that cannot start fails
  startup visibly before entering raw mode. The chosen path is reported.

## Shared sink and format

A small `rootle-trace` crate sits below the app and provider implementations;
it has no dependency on the provider trait or ratatui. The old exported
`rootle_provider::trace` function and every caller are removed in one cutover.

Public contract for integration:

- `start(path: &Path, options: TraceOptions) -> Result<TraceSession, TraceError>`.
- `TraceOptions { content: ContentPolicy, limits: Limits }`; policies are
  `Metadata` (default) and `Full`. Defaults: 64 MiB total, 100,000 events,
  256 KiB per record, a reserved terminal record, and a bounded queue.
- `enabled()`, `capture_content()`, `record(kind, &fields)` and lazy
  `record_with(kind, || fields)`; disabled producers do not build payloads.
- `operation_id() -> Option<OperationId>` allocates a correlation identity
  only while enabled; `in_operation(id, || work)` carries it through a
  worker's synchronous provider/HTTP calls. No mutation of provider payloads
  or application event-channel semantics is needed for correlation.
- `take_failure() -> Option<String>` reports a capture failure once.
  `TraceSession::finish(self)` drains and reports failure outside input/draw.
  Shutdown waiting is bounded; abandoned or failed capture is not complete.
- `panic_record(location, message, backtrace)` is best-effort, uses nonblocking
  admission during unwinding, marks capture incomplete and flushes with a
  bounded wait. It must not deadlock if a producer itself panics. Raw panic
  text/backtrace is included only under Full; location is metadata.
- `mark_incomplete(reason)` reports a producer-side capture failure (for
  example an unavailable stderr drainer) without pretending the log is complete.

Envelope: `schema_version`, monotonically ordered `seq`, `elapsed_us`,
`thread`, optional `operation_id`, `event`, `fields`. Sequence and timestamps
must agree with admission order under concurrent producers. Only the writer
touches the file; it flushes available batches, not only on clean shutdown.
Admission never waits for file I/O or for queue capacity. Overflow, record or
total caps, serialization errors and writer failure are explicit capture
failures. File mode is 0600 on Unix using exclusive creation.

Review hardened the strop pattern rather than copying its lifetime races:
the active recorder is an atomic Arc snapshot, captured before lazy payload
construction; content/panic state belongs to that recorder, not global flags.
Failed captures disable lazy producers. Panic flush acknowledgement follows
the actual panic record, not a flag observed while flushing an older batch.
Full stderr readers are cancellable/nonblocking and drain on shutdown only
within a bounded grace period, even if a descendant retained the pipe.

Closed event vocabulary:
`SessionStart`, `SessionEnd`, `TraceEnd`, `Input`, `Action`, `State`, `Render`,
`Resize`, `JobStarted`, `JobFinished`, `JobRejected`, `RpcMessage`,
`ProviderLifecycle`, `ProviderStderr`, `HttpRequest`, `HttpResponse`, `Cache`,
`Config`, `ExternalCommand`, `Error`, `Panic`.

Every successfully closed file ends with `trace_end`, including whether the
capture is complete and why. A crash/kill or I/O failure can prevent that
write; absence of a terminal marker itself means incomplete. Complete
capture does not mean that the application operation succeeded: session and
job outcomes retain their own result. No unbounded recording or quiet drops.

## Privacy contract

Metadata is diagnostic, not anonymous: repository identities, paths,
revisions, operation names and timing can still be sensitive. Files are
private; inspect them before sharing.

By default, printable key text, query/command text, message/status contents,
file/patch text, clipboard data, rendered glyphs and provider stderr are not
recorded. Record kinds, lengths, selected indices, request generations,
error kinds/status codes and cell/style fingerprints instead. Full mode
adds exact input, UI field/status text and visible cells/styles; it can
contain sensitive text typed or displayed in the application, including
provider stderr. This is explicit sensitive capture, not redaction.

Never automatically record authorization headers, environment values,
provider/editor command arguments, raw JSON-RPC bodies or HTTP bodies, even
in Full mode. HTTP records use method plus safe endpoint/resource metadata,
not credential-bearing userinfo, query strings or fragments. Do not use
`Debug` on complete actions, events, configs, responses or command vectors.
Remote error strings are omitted from metadata records; classify them and
capture their already-displayed UI form only under Full.

## Coverage and ownership

### Storage owner

Own `crates/trace/`: bounded sink, ordering/correlation, failure/panic/finish,
private creation and focused concurrency/lifecycle regressions. Main owns
workspace membership and release-order integration.

### Application owner

Own `crates/rootle/src/app/` plus diagnostic state accessors as needed in
existing components. Instrument the common input/action/event handlers,
including early returns and explicit stale-result rejection reasons. Record
mode/overlay/focus/filter/cursor/selection/viewport/request state after
transitions, not only in the headless driver. Instrument every app worker
with start/outcome/identity and correlated provider work. Remove old app
trace calls. Do not alter dispatch semantics to make logging easier.

Main owns CLI/main/headless and the common frame recorder. Cursor placement
changes in components are applied by Main after component accessor edits
finish, avoiding concurrent edits to shared files.

### Backend owner

Own `crates/stdio/` and `crates/github/`: actual RPC tx/rx byte lengths, IDs,
reader generation, partial/final/error classification, routing acceptance
and late/unknown/stale drops, timeout/cancel/rebuild/handshake/EOF; HTTP
requests/status/timing/body lengths without bodies; cache hit/miss,
revalidation, corruption fallback and eviction. Full-mode provider stderr
uses a bounded draining reader and preserves configured inherited stderr.
It must keep draining even when capture fails; terminate children before
joining their readers. Remove the GitHub client's old trace calls.

### Integration owner (Main)

Own command-line controls, session/panic/terminal lifecycle, headless and
interactive frame capture, provider composition/configuration, manager and
self-update diagnostics, publication dependency order, docs and validation.
Headless keeps a persistent TestBackend so consecutive frames expose the
same rendering lifecycle instead of rebuilding a terminal each time.
Frames include actual grid dimensions, requested cursor, elapsed draw time
and cell/style hash; Full adds bounded visible cell runs. Cursor capture
must reuse one placement helper rather than guessing from application state.

CLI code must return through session finalization instead of `process::exit`.
Log failures must not write into the active alternate screen; report them
through the app's status path and after restoration, with a failing final
result for an incomplete requested trace.

## Acceptance

1. Exercise real headless and PTY flows with tracing: input, action, state,
   consecutive renders, resize, worker/provider requests, commit inspection,
   navigation/backtracking and clean termination. The terminal remains clean.
2. Exercise a failing provider and stale/late response path; the trace names
   receipt versus acceptance/rejection with request identity and reason.
3. Exercise provider-manager/update failures without entering the TUI; session
   end and writer finalization still happen on errors.
4. Parse concurrent JSONL, check sequence/order and completion. Verify disabled
   lazy producers, exclusive 0600 creation, repeated lifecycle, caps/overflow,
   real writer failure and panic finalization using isolated regressions.
5. Seed sensitive input/content/arguments/RPC payloads: metadata excludes them;
   Full reveals only the explicitly allowed surfaces. Existing logs are intact
   after refusal. No log data leaks to stdout/headless framing.
6. Run Docker test and e2e gates after integration; keep the protocol model
   green if routing/lifecycle code is touched. Update investigation skills,
   contributor docs and changelog, and record remaining scope on the site.

## Deliberate boundaries

No editor mutation tape, deterministic external-world replay, always-on
telemetry, network log upload, interactive log viewer, or new generic tracing
framework. Rootle's existing headless script remains the driving surface;
JSONL supplies the evidence around it. Recorded-input extraction and provider
result replay are future, separately specified work, not implied by a trace.

This plan authorizes implementation and verification, not an automatic
version bump or release of a new workspace version.

## Exercised evidence

- Host fmt/clippy (`-D warnings`) and **311 Rust tests** passed.
- Headless/PTY e2e: **55 passed on host and in Docker**; the Docker
  test gate passed too. Provider conformance: **47 passed**.
- The provider-protocol Docker model gate remains green.
- All **seven** workspace packages built from their extracted `cargo package`
  archives, including the new sink and its consumers. Nothing was published.
- A real Full PTY capture contained 206 ordered records, including 31 actual
  render observations, 18 input events, 14 RPC records and a resize from
  100×30 to 80×22. The decoded saved cell grid equaled the actual pyte screen.
  Commit inspection, the Esc ladder, terminal restoration and completion passed.
- The live run exposed backend resizing without a separate observed input
  notification. Geometry-change records now originate at the draw boundary;
  the retained PTY regression defends that observation.
- Regression captures prove Metadata omits seeded input/source/argv/env/RPC
  secrets; Full adds the allowed visible input/cells/stderr only. Late provider
  replies name their rejection, and an inherited stderr pipe cannot hang exit.
- Real CLI checks covered existing-file refusal, explicit-path precedence,
  auto private paths, provider-command failure, update/network failure and a
  non-UTF-8 log filename. A kernel file-size limit caused a real writer error:
  the command returned failure, preserved JSON stdout, and did not claim a
  complete trace.
- Core regressions cover ordered concurrent producers, per-session privacy
  isolation, panic/finalization, caps and disabled lazy producers. Startup
  panic/file-system limits remain best-effort where no terminal marker can
  physically be written; absence is the incomplete verdict.
- Contributor and maintainer investigation instructions were updated; the
  site's development-only diagnostics and deferred replay notes were built
  and visually checked. No release/tag or version bump is part of this work.
