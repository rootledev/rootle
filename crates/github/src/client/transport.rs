//! Authenticated HTTP, conditional requests and error classification.

use super::GitHubClient;
use rootle_provider::{ErrorKind, ProviderError, ProviderResult};

pub(super) struct Page<Response> {
    pub body: Response,
    pub has_next: bool,
}

impl GitHubClient {
    pub(super) fn get<Response: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> ProviderResult<Response> {
        self.get_page(url).map(|page| page.body)
    }

    pub(super) fn get_page<Response: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> ProviderResult<Page<Response>> {
        let response = self.authenticated_get(url).send().map_err(classify_send)?;
        if !response.status().is_success() {
            return Err(classify_status(response));
        }
        let has_next = response
            .headers()
            .get(reqwest::header::LINK)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|links| links.split(',').any(|link| link.contains("rel=\"next\"")));
        let body = response
            .json()
            .map_err(|error| ProviderError::other(error.to_string()))?;
        Ok(Page { body, has_next })
    }

    fn authenticated_get(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        let request = self.http.get(url);
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    /// GET with an explicit Accept header (text-match fragments).
    pub(super) fn get_accept<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        accept: &str,
    ) -> ProviderResult<T> {
        let resp = self
            .authenticated_get(url)
            .header("Accept", accept)
            .send()
            .map_err(classify_send)?;
        if !resp.status().is_success() {
            return Err(classify_status(resp));
        }
        resp.json::<T>()
            .map_err(|e| ProviderError::other(e.to_string()))
    }

    pub(super) fn get_conditional<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> ProviderResult<Conditional<T>> {
        let mut req = self.authenticated_get(url);
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let resp = req.send().map_err(classify_send)?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(Conditional::NotModified);
        }
        if !status.is_success() {
            return Err(classify_status(resp));
        }
        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = resp
            .json::<T>()
            .map_err(|e| ProviderError::other(e.to_string()))?;
        Ok(Conditional::Fresh { body, etag })
    }
}

pub(super) enum Conditional<T> {
    NotModified,
    Fresh { body: T, etag: Option<String> },
}

// Classify a transport-level failure (plans/0008 §2).
pub(super) fn classify_send(e: reqwest::Error) -> ProviderError {
    let kind = if e.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::Network
    };
    ProviderError::new(kind, e.to_string())
}

/// Classify a non-2xx reply into the error taxonomy: 401/403 → auth
/// (403 with an exhausted rate limit is throttling, not auth), 404 →
/// not_found, 429 → rate_limited (Retry-After rides along), 5xx →
/// provider, anything else → other.
pub(super) fn classify_status(resp: reqwest::blocking::Response) -> ProviderError {
    let status = resp.status();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(std::time::Duration::from_secs);
    let remaining_zero = resp
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "0");
    let kind = match status.as_u16() {
        401 => ErrorKind::Auth,
        403 if remaining_zero => ErrorKind::RateLimited,
        403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::Provider,
        _ => ErrorKind::Other,
    };
    let error = ProviderError::new(kind, format!("HTTP {status}"));
    match retry_after {
        Some(d) => error.with_retry_after(d),
        None => error,
    }
}
