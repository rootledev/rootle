//! Explicit token environment, then the user's authenticated gh session.

/// Resolve the API token. Returns it with a label naming its source —
/// environment variable *names* are diagnostics, values never are.
pub(super) fn token() -> (Option<String>, &'static str) {
    for name in ["ROOTLE_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(name)
            && !value.is_empty()
        {
            return (Some(value), env_source(name));
        }
    }
    match gh_token() {
        Some(token) => (Some(token), "gh auth token"),
        None => (None, "none"),
    }
}

fn env_source(name: &str) -> &'static str {
    if name == "ROOTLE_TOKEN" {
        "env:ROOTLE_TOKEN"
    } else {
        "env:GITHUB_TOKEN"
    }
}

fn gh_token() -> Option<String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}
