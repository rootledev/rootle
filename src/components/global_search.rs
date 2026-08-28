//! Global search view (plans/0002-v0.2.md §1, §5): full-screen Zed-style
//! search that replaces the browser while open. Fields row on top
//! (query · scope · extension), results below with one block per hit —
//! full path, then preview lines under it. `␣ f` = file find,
//! `␣ g` = grep. `Enter` on a hit expands the results area into the
//! whole file at the match line (plans/0012 M2).
//!
//! Layout: this file is the component's state + public surface;
//! `keys.rs` handles input, `render.rs` draws, `model.rs` holds the
//! hit data model, `backend.rs` runs the real search on worker
//! threads, `mock.rs` is the offline producer.

use self::model::line_text;
use super::preview::{Preview, PreviewContent};
use super::vim_input::{SubMode, VimInput};
use crate::action::Action;
use crate::mode::Mode;
use ratatui::crossterm::cursor::SetCursorStyle;

mod backend;
mod facets;
mod grammar;
mod keys;
pub mod mock;
mod model;
mod render;

use facets::FacetId;

pub use backend::run_view_search;
pub use model::{RawHit, Scope, SearchHit, SearchKind, highlight_matches};

pub(crate) use backend::locate_in_blob;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Query,
    Scope,
    Extension,
    Facets,
    Results,
}

const FOCUS_ORDER: [Focus; 5] = [
    Focus::Query,
    Focus::Scope,
    Focus::Extension,
    Focus::Facets,
    Focus::Results,
];

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
    scroll: u16,
    pending: bool,
    error: Option<String>,
    submitted_once: bool,
    /// Result set is incomplete — provider-truncated (plans/0008 §4)
    /// or hits dropped past the render cap; shown in the results title.
    clipped: bool,
    /// plans/0012 M3: the committed facet chip (if any) — a local
    /// filter over the accumulated hits, composed with the `/`
    /// filter (facet first, then filter text). Cleared on a new
    /// search.
    facet: Option<FacetId>,
    /// Keyboard cursor in the chip row (index into `facets()`).
    facet_cursor: usize,
    /// Streamed hits past RENDER_CAP (v1.3): counted, not kept — the
    /// title's clipped chip covers them.
    dropped: usize,
    /// v1.3: when the provider's index was built (None = live/unknown)
    /// — shown next to the result count.
    index_as_of: Option<String>,
    /// Expanded full-file pane (plans/0012 M2): `Enter` on a hit swaps
    /// the results area for the hit's whole file at the match line;
    /// `Esc`/`h` folds back. The results list and its scroll survive
    /// underneath, untouched.
    expanded: Option<ExpandedFile>,
    /// Find-in-file over the expanded pane (`/`): the input lives
    /// here, matches + chips in the re-used `Preview`.
    find_input: VimInput,
    finding: bool,
    /// plans/0012 M1: hits the client-side grammar filter subtracted
    /// (a grammar-incapable backend over-served), and the tokens
    /// rootle couldn't express anywhere — both are title chips.
    client_filtered: usize,
    unfiltered: Vec<String>,
    /// plans/0016 M1a: off-default revisions on index-backed backends
    /// can't be searched — the title says what the scope really is.
    pub search_ref_note: Option<String>,
}

/// The expanded full-file pane (plans/0012 M2). The re-used browser
/// `Preview` does the rendering (numbered lines, cursor, gutter,
/// find-in-file); this holds the anchor hit it was opened for.
struct ExpandedFile {
    /// Snapshot of the hit at expand time — repo/sha identify the
    /// blob, `line` is the cursor anchor (refreshed by a lazy context
    /// landing while the blob is still in flight).
    hit: SearchHit,
    /// Set once the file content landed; later anchor refinements are
    /// ignored — the user may already have walked the cursor.
    loaded: bool,
    preview: Preview,
}

/// Max rendered hits for a streamed search (v1.3, plans/0011): past
/// it the view counts and clips instead of growing without bound. The
/// same number goes out on the wire as `limit` (v1.4 advisory).
const RENDER_CAP: usize = crate::provider::RENDER_BUDGET;

impl GlobalSearch {
    /// `Y` in the expanded file pane: its preview's copy target.
    pub fn expanded_copy_target(&mut self) -> Option<(String, usize)> {
        self.expanded
            .as_mut()
            .and_then(|exp| exp.preview.copy_target())
    }

    /// `:42` — jump the expanded file pane to a line (plans/0016 M1).
    /// Returns false when nothing is expanded.
    pub fn expanded_goto_line(&mut self, line: u32) -> bool {
        match &mut self.expanded {
            Some(exp) => {
                exp.preview.set_cursor_line(line);
                true
            }
            None => false,
        }
    }
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
            hits: Vec::new(),
            filter: VimInput::transient(),
            clipped: false,
            dropped: 0,
            index_as_of: None,
            client_filtered: 0,
            unfiltered: vec![],
            search_ref_note: None,
            filtering: false,
            pre_filter: String::new(),
            filter_value: String::new(),
            facet: None,
            facet_cursor: 0,
            selected: 0,
            scroll: 0,
            pending: false,
            error: None,
            submitted_once: false,
            expanded: None,
            find_input: VimInput::transient(),
            finding: false,
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

    /// Hits surviving the committed facet (plans/0012 M3) and the
    /// committed `/` filter (path or preview text, case-insensitive
    /// substring — same rule as Pane::visible). Facet first, then the
    /// filter text; the two compose.
    fn visible(&self) -> Vec<&SearchHit> {
        let needle = self.filter_value.to_lowercase();
        self.hits
            .iter()
            .filter(|h| self.facet.as_ref().is_none_or(|f| f.matches(h)))
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

    /// Expand a hit into the full-file pane (plans/0012 M2). Real
    /// hits (sha) open as a loading placeholder and ask App for the
    /// blob — cache-first, so the lazy context usually warmed it;
    /// mock hits (body, no sha) render inline. The returned action is
    /// the fetch, when one is needed.
    fn expand_hit(&mut self, hit: &SearchHit) -> Action {
        let mut preview = Preview::focused();
        let action = if hit.sha.is_empty() {
            preview.set_bytes(&hit.path, hit.body.as_bytes());
            preview.title = file_title(hit);
            preview.set_cursor_line(hit.line);
            Action::Noop
        } else {
            preview.set_file_meta(&hit.path, None, &hit.sha);
            preview.title = file_title(hit);
            Action::LoadHitFile {
                hit: Box::new(hit.clone()),
            }
        };
        self.expanded = Some(ExpandedFile {
            hit: hit.clone(),
            loaded: hit.sha.is_empty(),
            preview,
        });
        action
    }

    /// Fold the file pane back to the results list (Esc/h). The list,
    /// the selection, and its scroll were never touched — collapse
    /// restores the exact view.
    fn collapse(&mut self) {
        self.expanded = None;
        self.finding = false;
        self.find_input.clear();
    }

    pub fn update(&mut self, action: &Action) {
        match action {
            Action::GlobalSearchSubmitted { .. } => {
                self.submitted_once = true;
                self.pending = true;
                self.error = None;
                self.hits.clear();
                self.dropped = 0;
                self.clipped = false;
                self.index_as_of = None;
                self.client_filtered = 0;
                self.unfiltered = vec![];
                self.facet = None; // a new search is a new facet set
                self.facet_cursor = 0;
                self.focus = Focus::Results;
                self.selected = 0;
                self.scroll = 0;
                self.collapse(); // a new search replaces the file pane
            }
            Action::GlobalSearchDelta { hits } => {
                self.append_hits(hits.clone());
                self.clamp_facet_cursor(); // chips grew — keep the cursor on one
            }
            Action::GlobalSearchResults {
                hits,
                clipped,
                index,
                client_filtered,
                unfiltered,
            } => {
                self.pending = false;
                self.clipped = *clipped || self.dropped > 0;
                self.index_as_of = index.clone();
                self.client_filtered = *client_filtered;
                self.unfiltered = unfiltered.clone();
                // A streamed final is metadata-only (empty hits) — the
                // accumulated set stands. A full set replaces it.
                if !hits.is_empty() {
                    self.hits = hits.clone();
                    self.dropped = 0;
                    self.selected = 0;
                    self.scroll = 0;
                }
                self.clamp_selection();
                self.clamp_facet_cursor();
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
                // The expanded pane anchors on this hit and its blob
                // is still in flight: adopt the located line so the
                // cursor lands right when the file lands (plans/0012
                // M2). Once loaded, the user owns the cursor.
                if let Some(exp) = &mut self.expanded
                    && !exp.loaded
                    && exp.hit.repo == *repo
                    && exp.hit.path == *path
                    && exp.hit.sha == *sha
                {
                    exp.hit.line = *line;
                }
            }
            // Expanded pane (plans/0012 M2): the whole blob landed,
            // already sanitized + highlighted by App. Identity match
            // drops fetches a later expand superseded.
            Action::HitFileLoaded {
                repo,
                path,
                sha,
                lang,
                lines,
            } => {
                if let Some(exp) = &mut self.expanded
                    && exp.hit.repo == *repo
                    && exp.hit.path == *path
                    && exp.hit.sha == *sha
                {
                    let anchor = exp.hit.line;
                    exp.preview.set_highlighted(path, lang, lines.clone());
                    exp.preview.title = file_title(&exp.hit);
                    exp.preview.set_cursor_line(anchor);
                    exp.loaded = true;
                }
            }
            Action::HitFileFailed { error, .. } => {
                if let Some(exp) = &mut self.expanded {
                    // Same surface as the browser's failed blob: the
                    // pane itself says what went wrong; Esc still
                    // folds back to the results.
                    exp.preview.content = PreviewContent::Text(format!(
                        "error: {}",
                        crate::app::provider_status(error)
                    ));
                    exp.loaded = true;
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

    /// Streamed batch (v1.3, plans/0011): merge same-file hits into
    /// their block, append the rest. Past RENDER_CAP hits are counted
    /// (`dropped`) and skipped.
    pub fn append_hits(&mut self, hits: Vec<SearchHit>) {
        for hit in hits {
            if self.hits.len() >= RENDER_CAP {
                self.dropped += 1;
                continue;
            }
            if let Some(existing) = self
                .hits
                .iter_mut()
                .find(|h| h.repo == hit.repo && h.path == hit.path)
            {
                existing.merge(hit);
            } else {
                self.hits.push(hit);
            }
        }
        self.clamp_selection();
    }

    /// Hits kept so far (streamed or replaced) — the modeline's live
    /// count while a search streams.
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// Modeline chip while the view is open (plans/0002 §2).
    pub fn effective_mode(&self) -> Mode {
        if self.finding {
            return Mode::Find;
        }
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
            Focus::Scope | Focus::Facets | Focus::Results => Mode::Browse,
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

/// File-pane title: `repo/path:line` — what you're looking at and
/// where the anchor sits (line omitted for unknown anchors, e.g.
/// path-only hits). Both halves sanitize at the boundary like any
/// other provider string.
fn file_title(hit: &SearchHit) -> String {
    let repo = crate::sanitize::sanitize_inline(&hit.repo);
    let path = crate::sanitize::sanitize_inline(&hit.path);
    if hit.line > 0 {
        format!("{repo}/{path}:{}", hit.line)
    } else {
        format!("{repo}/{path}")
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
            index: None,
            client_filtered: 0,
            unfiltered: vec![],
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
    fn enter_expands_hit_into_file_pane() {
        let mut v = view();
        submit(&mut v, "query"); // mock hits: body, no sha → render inline
        let action = v.handle_key(key(KeyCode::Enter));
        assert_eq!(action, Action::Noop, "mock hit needs no fetch");
        assert!(v.expanded.is_some(), "Enter expands the selected hit");
        let exp = v.expanded.as_ref().expect("expanded");
        assert_eq!(exp.hit.path, "src/widgets/list.rs");
        assert!(exp.loaded, "body hits render without a fetch");
        // The mock hit's line (42) exceeds its 9-line body — the
        // anchor clamps to the end of the file.
        assert_eq!(exp.preview.line(), Some(9));

        // Esc folds straight back; the selection is where it was.
        assert_eq!(v.handle_key(key(KeyCode::Esc)), Action::Noop);
        assert!(v.expanded.is_none());
        assert_eq!(
            v.selected_hit().map(|h| h.path.clone()),
            Some("src/widgets/list.rs".into())
        );
    }

    #[test]
    fn real_hit_expands_with_fetch_and_anchor() {
        let mut v = view();
        submit(&mut v, "query");
        let mut hit = SearchHit::plain(
            "owner/repo",
            "src/place.rs",
            3,
            vec![(3, "the needle".to_string())],
            1,
            String::new(),
        );
        hit.sha = "cafebab".into();
        v.hits = vec![hit.clone()];
        // Enter: loading placeholder + the fetch action.
        let action = v.handle_key(key(KeyCode::Enter));
        match &action {
            Action::LoadHitFile { hit: asked } => assert_eq!(asked.sha, "cafebab"),
            other => panic!("expected LoadHitFile, got {other:?}"),
        }
        assert!(!v.expanded.as_ref().expect("expanded").loaded);

        // Blob lands: highlighted content, cursor at the anchor line,
        // title says repo/path:line.
        v.update(&Action::HitFileLoaded {
            repo: hit.repo.clone(),
            path: hit.path.clone(),
            sha: hit.sha.clone(),
            lang: "rust".into(),
            lines: (1..=9)
                .map(|i| ratatui::text::Line::from(format!("line {i}")))
                .collect(),
        });
        let exp = v.expanded.as_ref().expect("expanded");
        assert!(exp.loaded);
        assert_eq!(exp.preview.line(), Some(hit.line), "cursor at the anchor");
        assert_eq!(
            exp.preview.title,
            format!("owner/repo/{}:{}", hit.path, hit.line)
        );

        // j walks the file cursor; Enter opens the editor on the hit.
        v.handle_key(key(KeyCode::Char('j')));
        assert_eq!(
            v.expanded.as_ref().expect("expanded").preview.line(),
            Some(hit.line + 1)
        );
        match v.handle_key(key(KeyCode::Enter)) {
            Action::OpenSearchHit(opened) => assert_eq!(opened.path, hit.path),
            other => panic!("expected OpenSearchHit, got {other:?}"),
        }
    }

    #[test]
    fn path_only_hit_expands_to_top_of_file() {
        let mut v = view();
        submit(&mut v, "query");
        // match_count 0, no known line (file-find shape).
        v.hits = vec![SearchHit::plain(
            "owner/repo",
            "docs/readme.md",
            0,
            vec![],
            0,
            String::new(),
        )];
        v.hits[0].sha = "beef00".into();
        v.handle_key(key(KeyCode::Enter));
        v.update(&Action::HitFileLoaded {
            repo: "owner/repo".into(),
            path: "docs/readme.md".into(),
            sha: "beef00".into(),
            lang: "markdown".into(),
            lines: (1..=4)
                .map(|i| ratatui::text::Line::from(format!("doc {i}")))
                .collect(),
        });
        let exp = v.expanded.as_ref().expect("expanded");
        assert_eq!(
            exp.preview.line(),
            Some(1),
            "unknown anchor falls back to top"
        );
        assert_eq!(
            exp.preview.title, "owner/repo/docs/readme.md",
            "no :0 suffix"
        );
        // Anchor past EOF clamps instead of panicking: fold back,
        // move the anchor, expand again.
        v.handle_key(key(KeyCode::Esc));
        v.hits[0].line = 99;
        v.handle_key(key(KeyCode::Enter));
        v.update(&Action::HitFileLoaded {
            repo: "owner/repo".into(),
            path: "docs/readme.md".into(),
            sha: "beef00".into(),
            lang: "markdown".into(),
            lines: (1..=4)
                .map(|i| ratatui::text::Line::from(format!("doc {i}")))
                .collect(),
        });
        assert_eq!(
            v.expanded.as_ref().expect("expanded").preview.line(),
            Some(4)
        );
    }

    #[test]
    fn file_pane_find_session_delegates_to_preview() {
        let mut v = view();
        submit(&mut v, "query");
        v.handle_key(key(KeyCode::Enter)); // expand (mock body)
        // `/` opens FIND over the file; the modeline chip follows.
        v.handle_key(key(KeyCode::Char('/')));
        assert!(v.finding);
        assert_eq!(v.effective_mode(), Mode::Find);
        for c in "mock".chars() {
            v.handle_key(key(KeyCode::Char(c)));
        }
        let exp = v.expanded.as_ref().expect("expanded");
        assert!(exp.preview.find_active(), "preview holds the session");
        // Enter commits; n/N step, Esc-h still collapses afterwards.
        v.handle_key(key(KeyCode::Enter));
        assert!(!v.finding);
        v.handle_key(key(KeyCode::Char('n')));
        v.handle_key(key(KeyCode::Char('h')));
        assert!(v.expanded.is_none(), "h folds the pane back");
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

    /// Tab to the chip row (from wherever focus sits — after submit
    /// that's Results, so the cycle wraps through the fields first).
    fn focus_facets(v: &mut GlobalSearch) {
        for _ in 0..super::FOCUS_ORDER.len() + 1 {
            if v.focus == Focus::Facets {
                return;
            }
            v.handle_key(key(KeyCode::Tab));
        }
        panic!("tab never reached the chip row");
    }

    #[test]
    fn tab_skips_the_chip_row_until_hits_land() {
        let mut v = view();
        // No hits yet: query → scope → extension → results, never
        // facets.
        for expected in [Focus::Scope, Focus::Extension, Focus::Results] {
            v.handle_key(key(KeyCode::Tab));
            assert_eq!(v.focus, expected);
        }
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Query, "wraps without stopping on facets");

        // Mock grep hits: one repo, rust + markdown.
        submit(&mut v, "query");
        focus_facets(&mut v);
        assert_eq!(v.facets().len(), 3);
    }

    #[test]
    fn facet_toggle_narrows_and_restores() {
        let mut v = view();
        submit(&mut v, "query"); // 4 hits: 3 rust + 1 markdown
        focus_facets(&mut v);
        // Chips: repo ratatui/ratatui·4, then rust·3, markdown·1.
        assert_eq!(v.facet_cursor, 0);
        v.handle_key(key(KeyCode::Char('l')));
        assert_eq!(v.facet_cursor, 1);
        v.handle_key(key(KeyCode::Enter)); // commit the rust facet
        assert_eq!(
            v.facet,
            Some(facets::FacetId {
                kind: facets::FacetKind::Lang,
                name: "rust".into(),
            })
        );
        assert_eq!(v.visible().len(), 3, "rust facet drops the markdown hit");
        v.handle_key(key(KeyCode::Enter)); // toggle the active chip off
        assert!(v.facet.is_none());
        assert_eq!(v.visible().len(), 4, "full accumulated set restored");
    }

    #[test]
    fn facet_survives_streamed_batches_and_counts_climb() {
        let mut v = view();
        submit(&mut v, "query");
        focus_facets(&mut v);
        v.handle_key(key(KeyCode::Char('l'))); // rust chip
        v.handle_key(key(KeyCode::Enter)); // commit
        // A late batch lands: two more rust files in a second repo.
        v.update(&Action::GlobalSearchDelta {
            hits: vec![
                SearchHit::plain(
                    "other/repo",
                    "src/new.rs",
                    7,
                    vec![(7, "let query = 1;".to_string())],
                    1,
                    String::new(),
                ),
                SearchHit::plain(
                    "other/repo",
                    "src/aux.rs",
                    9,
                    vec![(9, "let query = 2;".to_string())],
                    1,
                    String::new(),
                ),
            ],
        });
        // The facet applies to the growing set…
        assert_eq!(v.visible().len(), 5, "new rust hits pass the facet");
        // …and the chips re-count over everything accumulated.
        let chips = v.facets();
        let rust = chips
            .iter()
            .find(|c| c.id.kind == facets::FacetKind::Lang && c.id.name == "rust")
            .expect("rust chip");
        assert_eq!(rust.count, 5);
        // The cursor stayed on a real chip.
        assert!(v.facet_cursor < chips.len());
    }

    #[test]
    fn facet_composes_with_slash_filter() {
        let mut v = view();
        submit(&mut v, "query");
        focus_facets(&mut v);
        v.handle_key(key(KeyCode::Char('l'))); // rust chip
        v.handle_key(key(KeyCode::Enter)); // commit the rust facet
        // `/` then narrows inside the facet's set.
        v.handle_key(key(KeyCode::Tab)); // → results
        assert_eq!(v.focus, Focus::Results);
        v.handle_key(key(KeyCode::Char('/')));
        for c in "terminal".chars() {
            v.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(v.visible().len(), 1);
        assert_eq!(v.visible()[0].path, "src/terminal.rs");
        // Esc clears the text filter first — the facet survives.
        v.handle_key(key(KeyCode::Esc));
        assert_eq!(v.visible().len(), 3, "facet still committed");
    }

    #[test]
    fn esc_peels_filter_then_facet_then_closes() {
        let mut v = view();
        submit(&mut v, "query");
        focus_facets(&mut v);
        v.handle_key(key(KeyCode::Char('l')));
        v.handle_key(key(KeyCode::Enter)); // commit rust facet
        assert_eq!(
            v.handle_key(key(KeyCode::Esc)),
            Action::Noop,
            "first Esc clears the facet, not the view"
        );
        assert!(v.facet.is_none());
        assert_eq!(v.visible().len(), 4);
        assert_eq!(
            v.handle_key(key(KeyCode::Esc)),
            Action::CloseSearchView,
            "second Esc closes"
        );
    }

    #[test]
    fn new_search_resets_the_facet() {
        let mut v = view();
        submit(&mut v, "query");
        focus_facets(&mut v);
        v.handle_key(key(KeyCode::Enter)); // commit the repo facet
        assert!(v.facet.is_some());
        // Back to the query field (facets → results → query), then a
        // fresh search replaces the set.
        v.handle_key(key(KeyCode::Tab));
        v.handle_key(key(KeyCode::Tab));
        submit(&mut v, "other");
        assert!(v.facet.is_none(), "a new search is a new facet set");
        assert_eq!(v.facet_cursor, 0);
    }
}
