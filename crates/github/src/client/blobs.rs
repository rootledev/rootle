//! Immutable blobs, path-at-revision content and source archives.

use super::transport::{classify_send, classify_status};
use super::{API, GitHubClient, urlencoding};
use rootle_provider::{ProviderError, ProviderResult};

impl GitHubClient {
    /// File at a ref via the contents API: raw bytes + the git blob
    /// sha (the provider's content id).
    pub fn blob_at(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        ref_: Option<&str>,
    ) -> ProviderResult<(Vec<u8>, String)> {
        #[derive(serde::Deserialize)]
        struct Contents {
            content: String, // base64 with embedded newlines
            sha: String,
        }
        let mut url = format!("{API}/repos/{owner}/{repo}/contents/{path}");
        if let Some(r) = ref_ {
            url.push_str(&format!("?ref={}", urlencoding(r)));
        }
        let item: Contents = self.get(&url)?;
        use base64::Engine;
        let clean: String = item
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(clean)
            .map_err(|e| ProviderError::other(format!("contents base64: {e}")))?;
        Ok((bytes, item.sha))
    }

    /// The default branch's source as a gzip tarball (api.github.com
    /// 302s to codeload). Capped — a repo past this size is not a
    /// fallback candidate.
    pub fn source_tarball(&self, repo: &str) -> ProviderResult<Vec<u8>> {
        const CAP: u64 = 64 * 1024 * 1024;
        let url = format!("{API}/repos/{repo}/tarball");
        let mut req = self.http.get(&url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let mut resp = req.send().map_err(classify_send)?;
        if !resp.status().is_success() {
            return Err(classify_status(resp));
        }
        if let Some(len) = resp.content_length()
            && len > CAP
        {
            return Err(ProviderError::other(format!(
                "tarball too large for local grep ({len} bytes)"
            )));
        }
        let mut bytes = Vec::new();
        let mut capped = std::io::Read::take(&mut resp, CAP);
        std::io::Read::read_to_end(&mut capped, &mut bytes)
            .map_err(|e| ProviderError::other(e.to_string()))?;
        Ok(bytes)
    }

    /// Fetch a blob by git sha, cache-first (blobs are immutable).
    /// Files over 1 MiB are rejected — too heavy for a preview pane.
    pub fn fetch_blob(&self, owner: &str, repo: &str, sha: &str) -> ProviderResult<Vec<u8>> {
        if let Some(bytes) = crate::cache::read_blob(sha) {
            return Ok(bytes);
        }
        #[derive(serde::Deserialize)]
        struct BlobResponse {
            content: String,
            size: u64,
        }
        let url = format!("{API}/repos/{owner}/{repo}/git/blobs/{sha}");
        let blob: BlobResponse = self.get(&url)?;
        if blob.size > 1024 * 1024 {
            return Err(ProviderError::other(format!(
                "file too large to preview ({} bytes)",
                blob.size
            )));
        }
        use base64::Engine;
        let clean: String = blob
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(clean)
            .map_err(|e| ProviderError::other(e.to_string()))?;
        let _ = crate::cache::write_blob(sha, &bytes);
        Ok(bytes)
    }
}
