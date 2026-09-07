//! Authenticated HTTP, conditional requests and error classification.
//! Every request runs through `HttpTrace` when a trace is active:
//! method, safe host/resource, status, elapsed time and body byte
//! counts — never headers, bodies, query strings, URL userinfo or
//! reqwest error strings (those can embed credential-bearing URLs).

use super::GitHubClient;
use rootle_provider::{ErrorKind, ProviderError, ProviderResult};
use rootle_trace::EventKind;
use serde_json::json;
use std::time::Instant;

pub(super) struct Page<Response> {
    pub body: Response,
    pub has_next: bool,
}

/// Safe URL metadata for diagnostics: host (any userinfo stripped) and
/// the request-target path. Query strings, fragments and userinfo are
/// never recorded — search queries and refs ride the query string.
pub(super) struct UrlMeta {
    pub host: String,
    pub resource: String,
    pub has_query: bool,
}

pub(super) fn url_meta(url: &str) -> UrlMeta {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    // Strip userinfo: `user:token@host` keeps only the host.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let resource_end = tail.find(['?', '#']).unwrap_or(tail.len());
    UrlMeta {
        host: host.to_string(),
        resource: tail[..resource_end].to_string(),
        has_query: tail.contains('?'),
    }
}

/// Per-request trace context; absent while tracing is disabled, so no
/// URL parsing or timers are constructed on the disabled path.
pub(super) struct HttpTrace {
    host: String,
    resource: String,
    start: Instant,
}

impl HttpTrace {
    pub(super) fn begin(url: &str, auth: &str, conditional: bool) -> Option<Self> {
        if !rootle_trace::enabled() {
            return None;
        }
        let meta = url_meta(url);
        rootle_trace::record_with(EventKind::HttpRequest, || {
            json!({
                "host": meta.host,
                "method": "GET",
                "resource": meta.resource,
                "has_query": meta.has_query,
                "auth": auth,
                "conditional": conditional,
            })
        });
        Some(Self {
            host: meta.host,
            resource: meta.resource,
            start: Instant::now(),
        })
    }

    /// Completed exchange: status (0 body bytes when the body was not
    /// read, i.e. a non-2xx reply) and the elapsed wall time.
    pub(super) fn finish(&self, status: u16, bytes: u64) {
        rootle_trace::record_with(EventKind::HttpResponse, || {
            json!({
                "host": self.host,
                "resource": self.resource,
                "status": status,
                "elapsed_us": self.start.elapsed().as_micros() as u64,
                "bytes": bytes,
            })
        });
    }

    /// Transport failure: the classified kind only — reqwest error
    /// strings can carry the credential-bearing request URL.
    pub(super) fn fail(&self, kind: &str) {
        rootle_trace::record_with(EventKind::HttpResponse, || {
            json!({
                "host": self.host,
                "resource": self.resource,
                "error_kind": kind,
                "elapsed_us": self.start.elapsed().as_micros() as u64,
            })
        });
    }

    /// ETag revalidation outcome (304 means the cached body stands).
    pub(super) fn revalidated(&self, outcome: &str) {
        rootle_trace::record_with(EventKind::Cache, || {
            json!({
                "op": "revalidate",
                "resource": self.resource,
                "outcome": outcome,
            })
        });
    }

    /// A response rejected before its body was read (e.g. the tarball
    /// size cap firing on Content-Length).
    pub(super) fn rejected(&self, reason: &str) {
        rootle_trace::record_with(EventKind::HttpResponse, || {
            json!({
                "host": self.host,
                "resource": self.resource,
                "outcome": "rejected",
                "reason": reason,
            })
        });
    }
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
        let trace = HttpTrace::begin(url, self.auth_source, false);
        let response = match self.authenticated_get(url).send() {
            Ok(response) => response,
            Err(error) => {
                if let Some(trace) = &trace {
                    trace.fail(send_error_kind(&error));
                }
                return Err(classify_send(error));
            }
        };
        let status = response.status();
        if !status.is_success() {
            if let Some(trace) = &trace {
                trace.finish(status.as_u16(), 0);
            }
            return Err(classify_status(response));
        }
        let has_next = response
            .headers()
            .get(reqwest::header::LINK)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|links| links.split(',').any(|link| link.contains("rel=\"next\"")));
        // Read the body explicitly so the trace can count real bytes.
        let bytes = match response.bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(trace) = &trace {
                    trace.fail("body_read");
                }
                return Err(ProviderError::other(error.to_string()));
            }
        };
        if let Some(trace) = &trace {
            trace.finish(status.as_u16(), bytes.len() as u64);
        }
        let body = serde_json::from_slice(&bytes)
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
        let trace = HttpTrace::begin(url, self.auth_source, false);
        let resp = match self.authenticated_get(url).header("Accept", accept).send() {
            Ok(resp) => resp,
            Err(error) => {
                if let Some(trace) = &trace {
                    trace.fail(send_error_kind(&error));
                }
                return Err(classify_send(error));
            }
        };
        let status = resp.status();
        if !status.is_success() {
            if let Some(trace) = &trace {
                trace.finish(status.as_u16(), 0);
            }
            return Err(classify_status(resp));
        }
        let bytes = match resp.bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(trace) = &trace {
                    trace.fail("body_read");
                }
                return Err(ProviderError::other(error.to_string()));
            }
        };
        if let Some(trace) = &trace {
            trace.finish(status.as_u16(), bytes.len() as u64);
        }
        serde_json::from_slice(&bytes).map_err(|e| ProviderError::other(e.to_string()))
    }

    pub(super) fn get_conditional<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> ProviderResult<Conditional<T>> {
        let trace = HttpTrace::begin(url, self.auth_source, etag.is_some());
        let mut req = self.authenticated_get(url);
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let resp = match req.send() {
            Ok(resp) => resp,
            Err(error) => {
                if let Some(trace) = &trace {
                    trace.fail(send_error_kind(&error));
                }
                return Err(classify_send(error));
            }
        };
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            if let Some(trace) = &trace {
                trace.finish(304, 0);
                trace.revalidated("not_modified");
            }
            return Ok(Conditional::NotModified);
        }
        if !status.is_success() {
            if let Some(trace) = &trace {
                trace.finish(status.as_u16(), 0);
            }
            return Err(classify_status(resp));
        }
        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let bytes = match resp.bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(trace) = &trace {
                    trace.fail("body_read");
                }
                return Err(ProviderError::other(error.to_string()));
            }
        };
        if let Some(trace) = &trace {
            trace.finish(status.as_u16(), bytes.len() as u64);
            trace.revalidated("fresh");
        }
        let body =
            serde_json::from_slice(&bytes).map_err(|e| ProviderError::other(e.to_string()))?;
        Ok(Conditional::Fresh { body, etag })
    }
}

pub(super) enum Conditional<T> {
    NotModified,
    Fresh { body: T, etag: Option<String> },
}

/// The same taxonomy `classify_send` maps to, as a label.
fn send_error_kind(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() { "timeout" } else { "network" }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The privacy contract of HTTP tracing: a credential-bearing URL
    /// (userinfo token, query string with the user's search text,
    /// fragment) must reduce to host + plain path. Regression for
    /// plans/0030's never-recorded list.
    #[test]
    fn url_meta_strips_userinfo_query_and_fragment() {
        let meta = url_meta(
            "https://user:secret-token@example.com/repos/o/r/git/trees/main?recursive=1&ref=feature/x#frag",
        );
        assert_eq!(meta.host, "example.com");
        assert_eq!(meta.resource, "/repos/o/r/git/trees/main");
        assert!(meta.has_query);
        let recorded = format!("{}{}", meta.host, meta.resource);
        assert!(
            !recorded.contains("secret-token") && !recorded.contains("user:"),
            "userinfo must never survive into trace metadata"
        );
        assert!(
            !recorded.contains("recursive=1")
                && !recorded.contains("feature/x")
                && !recorded.contains("frag"),
            "query and fragment values must never survive into trace metadata"
        );
    }

    #[test]
    fn url_meta_handles_urls_without_query_or_path() {
        let meta = url_meta("https://api.github.com/repos/o/r");
        assert_eq!(meta.host, "api.github.com");
        assert_eq!(meta.resource, "/repos/o/r");
        assert!(!meta.has_query);
    }
}
