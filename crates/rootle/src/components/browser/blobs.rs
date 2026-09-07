//! Blob cache + the at-commit view (moved from browser.rs,
//! plans/0021 M2 — a pure move).

use super::Browser;
use ratatui::text::Line;

/// One fetched blob, cache entry for `Browser::blobs`.
pub(crate) struct CachedBlob {
    /// File name (syntax detection for restyle).
    pub(crate) name: String,
    /// Language label for the preview footer.
    pub(crate) lang: String,
    /// Sanitized raw text — the restyle source of truth.
    pub(crate) text: String,
    /// Highlighted lines under the current palette.
    pub(crate) lines: Vec<Line<'static>>,
}

impl Browser {
    pub fn show_at_commit(
        &mut self,
        sha: &str,
        name: &str,
        lang: &str,
        text: String,
        lines: Vec<Line<'static>>,
        band: Option<crate::components::preview::BandContext>,
    ) {
        self.note_commit_view();
        self.blobs.insert(
            sha.to_string(),
            CachedBlob {
                name: name.to_string(),
                lang: lang.to_string(),
                text,
                lines: lines.clone(),
            },
        );
        self.preview.set_highlighted(name, lang, lines);
        // The band: the path stays, the commit context dresses it.
        let path = name.split(" @ ").next().unwrap_or(name).to_string();
        self.preview.set_band(Some(path), band);
    }

    /// Esc from a commit view: the present-day blob re-renders from
    /// the in-memory cache (it was fetched moments ago — zero network).
    pub fn commit_view_close(&mut self) {
        let Some((path, sha)) = self.at_commit.take() else {
            return;
        };
        if let Some(c) = self.blobs.get(&sha) {
            let (lang, lines) = (c.lang.clone(), c.lines.clone());
            // The cached name carries the at-commit marker — the
            // present-day view's title is the plain path.
            self.preview.set_highlighted(&path, &lang, lines);
            self.preview.set_band(Some(path), None);
        } else {
            // Cache evicted under us: fall back to the blob path.
            self.preview
                .set_bytes(&path, b"reload the file to restore it");
        }
    }

    /// Blob arrived: store raw text + highlighted lines, refresh if
    /// still selected.
    pub fn blob_loaded(
        &mut self,
        sha: &str,
        name: &str,
        lang: &str,
        text: String,
        lines: Vec<Line<'static>>,
    ) {
        self.blobs.insert(
            sha.to_string(),
            CachedBlob {
                name: name.to_string(),
                lang: lang.to_string(),
                text,
                lines,
            },
        );
        self.pending_blobs.remove(sha);
        self.refresh_preview();
        // The header band (GitHub's file header): the full path rides
        // every file preview; at-commit context only from
        // show_at_commit.
        if let Some((path, s)) = self.selected_file()
            && s == sha
        {
            self.preview.set_band(Some(path), None);
        }
    }

    pub fn blob_failed(&mut self, sha: &str, message: &str) {
        // Cache the failure: re-selecting the file re-shows the error
        // (refresh_preview consults failed_blobs) instead of a
        // "loading…" placeholder nothing will resolve. No auto-retry
        // while the user keeps moving — explicit reload clears the
        // failure map (the retry path).
        self.pending_blobs.remove(sha);
        self.failed_blobs
            .insert(sha.to_string(), message.to_string());
        // Stale replies must not clobber the preview: the user may
        // have moved to another file while this fetch was failing.
        if let Some((_, selected)) = self.selected_file()
            && selected == sha
        {
            self.preview.content =
                crate::components::preview::PreviewContent::Text(format!("error: {message}"));
        }
    }

    /// Explicit reload (␣ r): failures clear AND the visible preview
    /// re-requests immediately — the retry path for a transient
    /// failure. (Clearing without refreshing left the error text on
    /// screen until the next cursor move.)
    pub fn retry_failed_blobs(&mut self) {
        self.failed_blobs.clear();
        self.refresh_preview();
    }
}
