//! Worker receipts and explicit acceptance/rejection observations.
use super::{describe_error, full, text_with};
use crate::event::AppEvent;
use rootle_trace::EventKind;
use serde_json::{Value, json};

/// Variant name for receipt/acceptance/rejection records. Kept in
/// lockstep with [`describe_event`]'s `"event"` field.
pub(crate) fn event_name(event: &AppEvent) -> &'static str {
    match event {
        AppEvent::SearchResults { .. } => "search_results",
        AppEvent::SearchFailed { .. } => "search_failed",
        AppEvent::OrgReposLoaded { .. } => "org_repos_loaded",
        AppEvent::OrgReposFailed { .. } => "org_repos_failed",
        AppEvent::TreeLoaded { .. } => "tree_loaded",
        AppEvent::TreeFailed { .. } => "tree_failed",
        AppEvent::BlobLoaded { .. } => "blob_loaded",
        AppEvent::BlobFailed { .. } => "blob_failed",
        AppEvent::GlobalSearchResults { .. } => "global_search_results",
        AppEvent::GlobalSearchDelta { .. } => "global_search_delta",
        AppEvent::GlobalSearchFailed { .. } => "global_search_failed",
        AppEvent::HitContextLoaded { .. } => "hit_context_loaded",
        AppEvent::HitContextMissing { .. } => "hit_context_missing",
        AppEvent::HitContextDebounceFired { .. } => "hit_context_debounce_fired",
        AppEvent::HitContextFailed { .. } => "hit_context_failed",
        AppEvent::HitFileLoaded { .. } => "hit_file_loaded",
        AppEvent::HitFileFailed { .. } => "hit_file_failed",
        AppEvent::CloneDone { .. } => "clone_done",
        AppEvent::CloneExpanded { .. } => "clone_expanded",
        AppEvent::RefsLoaded { .. } => "refs_loaded",
        AppEvent::RefsFailed { .. } => "refs_failed",
        AppEvent::LogLoaded { .. } => "log_loaded",
        AppEvent::LogFailed { .. } => "log_failed",
        AppEvent::CommitLoaded { .. } => "commit_loaded",
        AppEvent::BlameLoaded { .. } => "blame_loaded",
        AppEvent::BlameFailed { .. } => "blame_failed",
        AppEvent::BlobAtLoaded { .. } => "blob_at_loaded",
        AppEvent::BlobAtFailed { .. } => "blob_at_failed",
        AppEvent::UpdateAvailable { .. } => "update_available",
        AppEvent::DeclarationInstalled { .. } => "declaration_installed",
        AppEvent::DeclarationFailed { .. } => "declaration_failed",
        AppEvent::LastCommitLoaded { .. } => "last_commit_loaded",
    }
}

/// Worker-event identity: name, generations, repo/path/sha, counts
/// and classified errors — no hit, blob or message contents.
pub(super) fn describe_event(event: &AppEvent) -> Value {
    let full = full();
    match event {
        AppEvent::SearchResults { gen_id, items } => json!({
            "gen": *gen_id,
            "items": items.len(),
        }),
        AppEvent::SearchFailed { gen_id, error } => json!({
            "gen": *gen_id,
            "error": describe_error(error),
        }),
        AppEvent::OrgReposLoaded { org, repos } => json!({
            "org": org,
            "repos": repos.len(),
        }),
        AppEvent::OrgReposFailed { org, error } => json!({
            "org": org,
            "error": describe_error(error),
        }),
        AppEvent::TreeLoaded {
            owner,
            name,
            entries,
            truncated,
            branch,
        } => json!({
            "repo": format!("{owner}/{name}"),
            "entries": entries.len(),
            "truncated": truncated,
            "branch": branch,
        }),
        AppEvent::TreeFailed { owner, name, error } => json!({
            "repo": format!("{owner}/{name}"),
            "error": describe_error(error),
        }),
        AppEvent::BlobLoaded { sha, name, bytes } => json!({
            "sha": sha,
            "path": name,
            "bytes": bytes.len(),
        }),
        AppEvent::BlobFailed { sha, error } => json!({
            "sha": sha,
            "error": describe_error(error),
        }),
        AppEvent::GlobalSearchResults {
            gen_id,
            hits,
            clipped,
            index,
            client_filtered,
            unfiltered,
        } => json!({
            "gen": *gen_id,
            "hits": hits.len(),
            "clipped": clipped,
            "index_as_of": index.is_some(),
            "client_filtered": client_filtered,
            "unfiltered": unfiltered.len(),
        }),
        AppEvent::GlobalSearchDelta { gen_id, hits } => json!({
            "gen": *gen_id,
            "hits": hits.len(),
        }),
        AppEvent::GlobalSearchFailed { gen_id, error } => json!({
            "gen": *gen_id,
            "error": describe_error(error),
        }),
        AppEvent::HitContextLoaded {
            gen_id,
            repo,
            path,
            sha,
            line,
            preview,
            match_count,
            query,
        } => json!({
            "gen": *gen_id,
            "repo": repo,
            "path": path,
            "sha": sha,
            "line": line,
            "preview_lines": preview.len(),
            "matches": match_count,
            "query": text_with(full, query),
        }),
        AppEvent::HitContextMissing { gen_id, sha } => json!({
            "gen": *gen_id,
            "sha": sha,
        }),
        AppEvent::HitContextDebounceFired {
            timer_gen,
            hit,
            query,
        } => json!({
            "timer_gen": timer_gen,
            "repo": hit.repo,
            "path": hit.path,
            "sha": hit.sha,
            "query": text_with(full, query),
        }),
        AppEvent::HitContextFailed { gen_id, sha, error } => json!({
            "gen": *gen_id,
            "sha": sha,
            "error": describe_error(error),
        }),
        AppEvent::HitFileLoaded {
            gen_id,
            repo,
            path,
            sha,
            bytes,
        } => json!({
            "gen": *gen_id,
            "repo": repo,
            "path": path,
            "sha": sha,
            "bytes": bytes.len(),
        }),
        AppEvent::HitFileFailed { gen_id, sha, error } => json!({
            "gen": *gen_id,
            "sha": sha,
            "error": describe_error(error),
        }),
        AppEvent::CloneDone { ok, failed } => json!({
            "ok": ok.len(),
            "failed": failed.len(),
        }),
        AppEvent::CloneExpanded { repos, errors } => json!({
            "repos": repos.len(),
            "org_errors": errors.len(),
        }),
        AppEvent::RefsLoaded { repo, refs } => json!({
            "repo": repo,
            "branches": refs.branches.len(),
            "tags": refs.tags.len(),
        }),
        AppEvent::RefsFailed { repo, error } => json!({
            "repo": repo,
            "error": describe_error(error),
        }),
        AppEvent::LogLoaded {
            request,
            entries,
            truncated,
        } => json!({
            "request": request,
            "entries": entries.len(),
            "truncated": truncated,
        }),
        AppEvent::LogFailed { request, error } => json!({
            "request": request,
            "error": describe_error(error),
        }),
        AppEvent::CommitLoaded { request, detail } => json!({
            "repository": request.repository.as_str(),
            "revision": request.revision.as_str(),
            "gen": request.generation,
            "outcome": if detail.is_ok() { "ok" } else { "err" },
            "error": match detail {
                Ok(_) => Value::Null,
                Err(error) => describe_error(error),
            },
        }),
        AppEvent::BlameLoaded { path, ranges } => json!({
            "path": path,
            "ranges": ranges.len(),
        }),
        AppEvent::BlameFailed { path, error } => json!({
            "path": path,
            "error": describe_error(error),
        }),
        AppEvent::BlobAtLoaded {
            path,
            ref_,
            sha,
            bytes,
            ..
        } => json!({
            "path": path,
            "ref": ref_,
            "sha": sha,
            "bytes": bytes.len(),
        }),
        AppEvent::BlobAtFailed { path, error } => json!({
            "path": path,
            "error": describe_error(error),
        }),
        AppEvent::UpdateAvailable { tag, toast } => json!({
            "tag": tag,
            "toast": toast,
        }),
        AppEvent::DeclarationInstalled { name } => json!({
            "name": name,
        }),
        AppEvent::DeclarationFailed { name, error } => json!({
            "name": name,
            "error": text_with(full, error),
        }),
        AppEvent::LastCommitLoaded { repo, path, entry } => json!({
            "repo": repo,
            "path": path,
            "entry": entry.is_some(),
        }),
    }
}

/// Worker-event receipt, before any guard runs.
pub(crate) fn record_event_received(event: &AppEvent) {
    rootle_trace::record_with(EventKind::JobFinished, || {
        let mut fields = describe_event(event);
        fields["event"] = json!(event_name(event));
        fields["received"] = json!(true);
        fields
    });
}

/// The event landed and was applied.
pub(crate) fn record_event_accepted(name: &'static str) {
    rootle_trace::record_with(EventKind::JobFinished, || {
        json!({
            "event": name,
            "accepted": true,
        })
    });
}

/// The event arrived but an identity guard dropped it. `detail` is
/// built lazily — nothing is constructed while tracing is disabled.
pub(crate) fn record_event_rejected(
    name: &'static str,
    reason: &'static str,
    detail: impl FnOnce() -> Value,
) {
    rootle_trace::record_with(EventKind::JobRejected, || {
        let mut fields = detail();
        fields["event"] = json!(name);
        fields["reason"] = json!(reason);
        fields
    });
}
