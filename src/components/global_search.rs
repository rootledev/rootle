//! Global search view (plans/0002-v0.2.md §1, §5): full-screen Zed-style
//! search that replaces the browser while open. Fields row on top
//! (query · scope · extension), results below with one block per hit —
//! full path, then preview lines under it. `␣ f` = file find,
//! `␣ g` = grep.
//!
//! Layout: this file is the component's state + public surface;
//! `keys.rs` handles input, `render.rs` draws, `model.rs` holds the
//! hit data model, `backend.rs` runs the real search on worker
//! threads, `mock.rs` is the offline producer.

use self::model::line_text;
use super::vim_input::{SubMode, VimInput};
use crate::action::Action;
use crate::mode::Mode;
use ratatui::crossterm::cursor::SetCursorStyle;

mod backend;
mod keys;
pub mod mock;
mod model;
mod render;

pub use backend::run_view_search;
pub use model::{RawHit, Scope, SearchHit, SearchKind, highlight_matches};

pub(crate) use backend::locate_in_blob;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Query,
    Scope,
    Extension,
    Results,
}

const FOCUS_ORDER: [Focus; 4] = [Focus::Query, Focus::Scope, Focus::Extension, Focus::Results];

pub struct GlobalSearch {
    kind: SearchKind,
    /// Browser's open repo ("owner/name"); gates the Repo scope.
    repo: Option<String>,
    /// Browser's selected org; gates the Org scope.
    org: Option<String>,
    pub query: VimInput,
    extension: VimInput,
    scope: Scope,
    focus: Focus,
    /// Scope radio popup open.
    scope_popup: bool,
    /// Cursor inside the scope popup (index into `scope_items`).
    scope_cursor: usize,
    /// Scope when the popup opened; Esc reverts to it (the radio
    /// follows the cursor live, so cancel needs the original).
    scope_pre_popup: Scope,
    hits: Vec<SearchHit>,
    /// `/` transient filter over the results (path + preview text).
    filter: VimInput,
    filtering: bool,
    pre_filter: String,
    filter_value: String,
    /// Selected hit within the visible set.
    selected: usize,
    /// Line scroll offset of the results area (J/K free scroll).
    scroll: u16,
    pending: bool,
    error: Option<String>,
    submitted_once: bool,
    /// Result set is incomplete — provider-truncated or client-capped
    /// at HIT_CAP (plans/0008 §4); shown in the results title.
    clipped: bool,
}

impl GlobalSearch {
    /// The scope waterfalls from the current browser context: an open
    /// repo defaults to Repo, otherwise a selected org to Org,
    /// otherwise Global. A persisted scope (state.json) wins when its
    /// context is still available; same for the extension field.
    pub fn new(
        kind: SearchKind,
        repo: Option<String>,
        org: Option<String>,
        persisted_scope: Option<Scope>,
        persisted_extension: Option<String>,
    ) -> Self {
        let waterfall = if repo.is_some() {
            Scope::Repo
        } else if org.is_some() {
            Scope::Org
        } else {
            Scope::Global
        };
        let enabled = |s: Scope| match s {
            Scope::Repo => repo.is_some(),
            Scope::Org => org.is_some(),
            Scope::Global => true,
        };
        let scope = persisted_scope.filter(|s| enabled(*s)).unwrap_or(waterfall);
        let mut extension = VimInput::new();
        if let Some(ext) = persisted_extension.filter(|e| !e.is_empty()) {
            extension.prefill(&ext); // replaceable: typing starts fresh
        }
        GlobalSearch {
            kind,
            scope,
            repo,
            org,
            query: VimInput::new(),
            extension,
            focus: Focus::Query,
            scope_popup: false,
            scope_cursor: 0,
            scope_pre_popup: Scope::Global,
            hits: vec![],
            filter: VimInput::transient(),
            filtering: false,
            pre_filter: String::new(),
            filter_value: String::new(),
            selected: 0,
            scroll: 0,
            clipped: false,
            pending: false,
            error: None,
            submitted_once: false,
        }
    }

    pub fn kind(&self) -> SearchKind {
        self.kind
    }

    /// Current scope (for persistence on submit).
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// Current extension field value (for persistence on submit).
    pub fn extension_value(&self) -> String {
        self.extension.value()
    }

    /// (scope, enabled) radio rows for the scope popup.
    fn scope_items(&self) -> [(Scope, bool); 3] {
        [
            (Scope::Repo, self.repo.is_some()),
            (Scope::Org, self.org.is_some()),
            (Scope::Global, true),
        ]
    }

    fn scope_label(&self) -> String {
        match self.scope {
            Scope::Repo => match &self.repo {
                Some(repo) => format!("repo:{repo}"),
                None => "repo: —".into(),
            },
            Scope::Org => match &self.org {
                Some(org) => format!("org:{org}"),
                None => "org: —".into(),
            },
            Scope::Global => "global".into(),
        }
    }

    /// Modeline context: effective query summary (plans/0002 §2).
    pub fn context(&self) -> String {
        let what = match self.kind {
            SearchKind::FileFind => "find",
            SearchKind::Grep => "grep",
        };
        let mut ctx = format!("{what} · {}", self.scope_label());
        if !self.extension.value().is_empty() {
            ctx.push_str(&format!(" · ext:{}", self.extension.value()));
        }
        ctx
    }

    /// Hits surviving the committed `/` filter (path or preview text,
    /// case-insensitive substring — same rule as Pane::visible).
    fn visible(&self) -> Vec<&SearchHit> {
        let needle = self.filter_value.to_lowercase();
        self.hits
            .iter()
            .filter(|h| {
                needle.is_empty()
                    || h.path.to_lowercase().contains(&needle)
                    || h.preview
                        .iter()
                        .any(|(_, line)| line_text(line).to_lowercase().contains(&needle))
            })
            .collect()
    }

    pub fn selected_hit(&self) -> Option<&SearchHit> {
        self.visible().get(self.selected).copied()
    }

    /// v1.1 lazy context (plans/0006 §1): the cursor sits on a hit
    /// with a sha but no preview lines — ask for its blob context.
    pub fn context_request(&self) -> Option<Action> {
        let hit = self.selected_hit()?;
        if !hit.preview.is_empty() || hit.sha.is_empty() {
            return None;
        }
        Some(Action::LoadHitContext {
            hit: Box::new(hit.clone()),
            query: self.query.value(),
        })
    }

    pub fn update(&mut self, action: &Action) {
        match action {
            Action::GlobalSearchSubmitted { .. } => {
                self.submitted_once = true;
                self.pending = true;
                self.error = None;
                self.focus = Focus::Results;
                self.selected = 0;
                self.scroll = 0;
            }
            Action::GlobalSearchResults { hits, clipped } => {
                self.pending = false;
                self.clipped = *clipped;
                self.hits = hits.clone();
                self.selected = 0;
                self.scroll = 0;
                self.clamp_selection();
            }
            Action::GlobalSearchFailed { error } => {
                self.pending = false;
                self.error = Some(crate::app::provider_status(error));
                self.hits = vec![];
            }
            // v1.1 lazy context landed (plans/0006 §1): merge by
            // identity — the hit list may have been filtered/reordered
            // since the fetch started.
            Action::HitContextLoaded {
                repo,
                path,
                sha,
                line,
                preview,
                match_count,
            } => {
                let target = self
                    .hits
                    .iter_mut()
                    .find(|h| &h.repo == repo && &h.path == path && &h.sha == sha);
                if let Some(hit) = target
                    && !preview.is_empty()
                {
                    hit.line = *line;
                    hit.preview = preview.clone();
                    hit.match_count = *match_count;
                    hit.stale = false;
                }
            }
            // v1.2 (plans/0008 §4): the blob answered but the match
            // text isn't in it — flip to unlocatable (never
            // self-heals) instead of rendering stale forever.
            Action::HitContextMissing { sha } => {
                for hit in self.hits.iter_mut().filter(|h| &h.sha == sha) {
                    hit.stale = false;
                    hit.unlocatable = true;
                }
            }
            _ => {}
        }
    }

    /// Modeline chip while the view is open (plans/0002 §2).
    pub fn effective_mode(&self) -> Mode {
        if self.filtering {
            return Mode::Search;
        }
        match self.focus {
            Focus::Query => match self.query.submode {
                SubMode::Insert => Mode::Insert,
                SubMode::Normal => Mode::Normal,
            },
            Focus::Extension => match self.extension.submode {
                SubMode::Insert => Mode::Insert,
                SubMode::Normal => Mode::Normal,
            },
            Focus::Scope | Focus::Results => Mode::Browse,
        }
    }

    /// Cursor shape for the focused text input (PLAN.md §5); hidden
    /// for the scope field and results.
    pub fn cursor_style(&self) -> Option<SetCursorStyle> {
        let input = match self.focus {
            Focus::Query => &self.query,
            Focus::Extension => &self.extension,
            _ => return None,
        };
        Some(match input.submode {
            SubMode::Insert => SetCursorStyle::SteadyBar,
            SubMode::Normal => SetCursorStyle::SteadyBlock,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::mock;
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn view() -> GlobalSearch {
        GlobalSearch::new(
            SearchKind::Grep,
            Some("ratatui/ratatui".into()),
            Some("ratatui".into()),
            None,
            None,
        )
    }

    fn submit(view: &mut GlobalSearch, query: &str) {
        for c in query.chars() {
            view.handle_key(key(KeyCode::Char(c)));
        }
        let action = view.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, Action::GlobalSearchSubmitted { .. }));
        view.update(&action);
        view.update(&Action::GlobalSearchResults {
            hits: mock::hits(SearchKind::Grep, query, ""),
            clipped: false,
        });
    }

    #[test]
    fn tab_cycles_all_four_focus_targets() {
        let mut v = view();
        assert_eq!(v.focus, Focus::Query);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Scope);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Extension);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Results);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Query);
        v.handle_key(key(KeyCode::BackTab));
        assert_eq!(v.focus, Focus::Results);
    }

    #[test]
    fn enter_in_query_submits_and_focuses_results() {
        let mut v = view();
        submit(&mut v, "query");
        assert_eq!(v.focus, Focus::Results);
        assert_eq!(v.hits.len(), 4);
    }

    #[test]
    fn scope_popup_radio_follows_cursor_and_esc_reverts() {
        let mut v = view();
        v.handle_key(key(KeyCode::Tab)); // scope focused
        v.handle_key(key(KeyCode::Enter));
        assert!(v.scope_popup);
        // Radio follows the cursor down the waterfall: repo → org → global.
        v.handle_key(key(KeyCode::Char('j')));
        assert_eq!(v.scope, Scope::Org);
        v.handle_key(key(KeyCode::Char('j')));
        assert_eq!(v.scope, Scope::Global);
        v.handle_key(key(KeyCode::Esc)); // revert to the pre-popup scope
        assert!(!v.scope_popup);
        assert_eq!(v.scope, Scope::Repo);

        // Enter commits wherever the radio stands.
        v.handle_key(key(KeyCode::Enter));
        v.handle_key(key(KeyCode::Char('j')));
        v.handle_key(key(KeyCode::Char('j')));
        v.handle_key(key(KeyCode::Enter));
        assert!(!v.scope_popup);
        assert_eq!(v.scope, Scope::Global);
    }

    #[test]
    fn repo_and_org_scopes_disabled_without_context() {
        let mut v = GlobalSearch::new(SearchKind::FileFind, None, None, None, None);
        assert_eq!(v.scope, Scope::Global);
        v.handle_key(key(KeyCode::Tab));
        v.handle_key(key(KeyCode::Enter)); // open popup
        v.handle_key(key(KeyCode::Char('j'))); // wraps: repo + org skipped
        assert_eq!(v.scope_cursor, 2); // global stays the only target
        v.handle_key(key(KeyCode::Enter));
        assert_eq!(v.scope, Scope::Global);
    }

    #[test]
    fn scope_waterfalls_from_browser_context() {
        // Repo open → Repo; only org → Org; nothing → Global.
        let v = GlobalSearch::new(
            SearchKind::Grep,
            Some("ratatui/ratatui".into()),
            Some("ratatui".into()),
            None,
            None,
        );
        assert_eq!(v.scope, Scope::Repo);
        let v = GlobalSearch::new(SearchKind::Grep, None, Some("ratatui".into()), None, None);
        assert_eq!(v.scope, Scope::Org);
        assert_eq!(v.scope_label(), "org:ratatui");
        let v = GlobalSearch::new(SearchKind::Grep, None, None, None, None);
        assert_eq!(v.scope, Scope::Global);
    }

    #[test]
    fn slash_filter_narrows_and_esc_restores() {
        let mut v = view();
        submit(&mut v, "query");
        assert_eq!(v.focus, Focus::Results);
        v.handle_key(key(KeyCode::Char('/')));
        assert!(v.filtering);
        for c in "terminal".chars() {
            v.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(v.visible().len(), 1);
        assert_eq!(v.visible()[0].path, "src/terminal.rs");
        v.handle_key(key(KeyCode::Esc)); // cancel → pre-filter
        assert_eq!(v.visible().len(), 4);
    }

    #[test]
    fn effective_mode_follows_focus_and_submode() {
        let mut v = view();
        assert_eq!(v.effective_mode(), Mode::Insert);
        v.query.submode = SubMode::Normal;
        assert_eq!(v.effective_mode(), Mode::Normal);
        v.handle_key(key(KeyCode::Tab)); // scope
        assert_eq!(v.effective_mode(), Mode::Browse);
    }

    #[test]
    fn esc_closes_from_results() {
        let mut v = view();
        submit(&mut v, "query");
        let action = v.handle_key(key(KeyCode::Esc));
        assert_eq!(action, Action::CloseSearchView);
    }

    #[test]
    fn enter_on_hit_emits_open() {
        let mut v = view();
        submit(&mut v, "query");
        let action = v.handle_key(key(KeyCode::Enter));
        match action {
            Action::OpenSearchHit(hit) => assert_eq!(hit.path, "src/widgets/list.rs"),
            other => panic!("expected OpenSearchHit, got {other:?}"),
        }
    }

    #[test]
    fn scope_field_cycles_with_vim_motions() {
        let mut v = view();
        v.handle_key(key(KeyCode::Tab)); // scope focused
        assert_eq!(v.scope, Scope::Repo);
        v.handle_key(key(KeyCode::Char('j'))); // repo → org, no popup
        assert_eq!(v.scope, Scope::Org);
        v.handle_key(key(KeyCode::Char('j'))); // org → global
        assert_eq!(v.scope, Scope::Global);
        assert!(!v.scope_popup);
        v.handle_key(key(KeyCode::Char('k'))); // back to org
        assert_eq!(v.scope, Scope::Org);
        // Disabled scopes are skipped when no context is open.
        let mut v = GlobalSearch::new(SearchKind::Grep, None, None, None, None);
        v.handle_key(key(KeyCode::Tab));
        v.handle_key(key(KeyCode::Char('k')));
        assert_eq!(v.scope, Scope::Global);
    }
}
