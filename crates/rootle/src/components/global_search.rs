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

mod results;

mod pane;
use pane::ExpandedFile;
pub use pane::YankTarget;

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
pub use model::{RawHit, Scope, SearchHit, SearchKind, chip_line, highlight_matches};

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
    /// Blame lens state for the expanded pane (0019 parity); the
    /// marks render in its Preview.
    blame: Option<crate::components::browser::BlameState>,
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

/// Max rendered hits for a streamed search (v1.3, plans/0011): past
/// it the view counts and clips instead of growing without bound. The
/// same number goes out on the wire as `limit` (v1.4 advisory).
const RENDER_CAP: usize = rootle_provider::RENDER_BUDGET;

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
            hits: Vec::new(),
            filter: VimInput::transient(),
            clipped: false,
            dropped: 0,
            index_as_of: None,
            client_filtered: 0,
            unfiltered: vec![],
            search_ref_note: None,
            blame: None,
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
mod tests;
