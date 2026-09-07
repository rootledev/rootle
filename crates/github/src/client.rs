//! GitHub client composition. Resource operations live in sibling modules;
//! only construction and shared session state belong in this file.

mod auth;
mod blame;
mod blobs;
mod history;
mod refs;
mod search;
mod transport;
mod trees;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const API: &str = "https://api.github.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct GitHubClient {
    http: reqwest::blocking::Client,
    token: Option<String>,
    blame_cache: Mutex<HashMap<blame::BlameCacheKey, Vec<rootle_provider::BlameRange>>>,
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubClient {
    pub fn new() -> Self {
        Self::build(auth::token())
    }

    /// No environment or credential process; useful for anonymous browsing.
    pub fn anonymous() -> Self {
        Self::build(None)
    }

    fn build(token: Option<String>) -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("rootle/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("construct GitHub HTTP client");
        Self {
            http,
            token,
            blame_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_anonymous(&self) -> bool {
        self.token.is_none()
    }
}

/// Percent-encode UTF-8 bytes, not Unicode scalar values. The latter
/// silently corrupts non-ASCII queries and revision names.
fn urlencoding(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            write!(&mut encoded, "%{byte:02X}").expect("write to a String");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding_preserves_unicode_and_reserved_characters() {
        assert_eq!(urlencoding("文字 +&"), "%E6%96%87%E5%AD%97%20%2B%26");
        assert_eq!(urlencoding("owner/repo"), "owner/repo");
    }
}
