//! Post-transition state shared by terminal and headless drivers.
use super::super::App;
use super::{entry_kind, full, opt_text_with};
use rootle_trace::EventKind;
use serde_json::{Value, json};

/// Post-transition state observation, shared by the app's own
/// handlers and the frame recorder (`App::record_trace_state`).
pub(crate) fn record_state(app: &App, reason: &'static str) {
    rootle_trace::record_with(EventKind::State, || describe_state(app, reason));
}

fn describe_state(app: &App, reason: &'static str) -> Value {
    let full = full();
    let browser = app.browser.diagnostics();
    json!({
        "reason": reason,
        "mode": app.effective_mode().chip(),
        "base_mode": app.mode.chip(),
        "overlays": {
            "popup": app.popup.is_some(),
            "search_view": app.search_view.is_some(),
            "help": app.help.is_some(),
            "command_line": app.command_line.is_some(),
            "settings": app.settings.is_some(),
            "wizard": app.wizard.is_some(),
            "refs_popup": app.refs_popup.is_some(),
            "consent": app.consent.is_some(),
        },
        "popup":app.popup.as_ref().map(|popup|popup.diagnostics(full)),
        "command_line":app.command_line.as_ref().map(|command|command.diagnostics(full)),
        "refs":app.refs_popup.as_ref().map(|popup|popup.diagnostics(full)),
        "help":app.help.as_ref().map(|popup|popup.diagnostics(full)),
        "settings":app.settings.as_ref().map(|popup|popup.diagnostics(full)),
        "clone":app.wizard.as_ref().map(|wizard|wizard.diagnostics(full)),
        "browser": {
            "focus": browser.focus,
            "columns":browser.columns,
            "visual": browser.visual,
            "marks": browser.marks,
            "blobs": {
                "cached": browser.cached_blobs,
                "pending": browser.pending_blobs,
                "failed": browser.failed_blobs,
            },
            "ref": app.browser.current_ref(),
            "branch": app.browser.branch(),
            "history": browser.history.map(|h| json!({
                "entries": h.entries,
                "visible": h.visible,
                "selected": h.selected,
                "loading": h.loading,
                "truncated": h.truncated,
                "path": app.browser.history_path(),
            })),
            "blame": browser.blame.map(|b| json!({
                "loading": b.loading,
                "ranges": b.ranges,
            })),
            "at_commit": app.browser.at_commit_view(),
            "commit": app.commit_surface(),
            "selected_kind": app.browser.selected_kind().map(entry_kind),
            "preview": {
                "line": app.browser.preview_line(),
                "lines": app.browser.preview.text_line_count(),
                "find": app.browser.preview.find_active(),
                "visual": app.browser.preview.visual_range().is_some(),
                "blaming": app.browser.preview.blaming(),
            },
            "filter_input":app.browser.filter_input.diagnostics(full),
            "find_input":app.browser.find_input.diagnostics(full),
        },
        "search": app.search_view.as_ref().map(|view| {
            let d = view.diagnostics();
            json!({
                "kind": d.kind,
                "input":view.query.diagnostics(full),
                "scope": d.scope,
                "focus": d.focus,
                "selected": d.selected,
                "hits": d.hits,
                "dropped": d.dropped,
                "clipped": d.clipped,
                "pending": d.pending,
                "filtering": d.filtering,
                "finding": d.finding,
                "expanded": d.expanded,
                "query_len": d.query_len,
                "filter_len": d.filter_len,
                "error": d.error_len.map(|len| json!(len)).unwrap_or(Value::Null),
            })
        }),
        "generations": {
            "search": app.search_gen,
            "view": app.view_gen,
            "commit": app.commit_generation,
            "debounce": app
                .context_debounce_gen
                .load(std::sync::atomic::Ordering::Relaxed),
        },
        "pending_context_sha": app.pending_context_sha,
        "status": opt_text_with(full, &app.status),
        "degraded": opt_text_with(full, &app.degraded),
        "update_tag": app.update_tag,
        "should_quit": app.should_quit,
        "offline": app.offline,
    })
}
