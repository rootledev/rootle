//! The install-reference grammar: what `rootle provider install`
//! accepts and how it resolves to repo, short name, and pin.

use super::{ManagerError, Result};

/// A resolved install reference (the grammar from the plan):
/// `gitlab` | `owner/repo` | `https://github.com/owner/repo`, each
/// optionally `@tag`-pinned — or a plain-HTTP URL naming the platform
/// tarball directly (plans/0014 #1a: arbitrary artifact hosts).
#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    /// owner/repo — or the full URL for a plain-HTTP install.
    pub repo: String,
    /// The bare name (repo stem minus the rootle- prefix, or the stem
    /// itself when unprefixed).
    pub name: String,
    pub tag: Option<String>,
    /// Plain-HTTP source: the tarball URL when the reference is not a
    /// releases-API source. Install-and-pin — `update`/`upgrade`
    /// never track these (plans/0014 #1b).
    pub tarball: Option<String>,
}

impl Ref {
    pub fn parse(input: &str) -> Result<Ref> {
        let input = input.trim();
        if input.is_empty() {
            return Err(ManagerError::User("empty provider reference".into()));
        }
        let (path, tag) = match input.split_once('@') {
            Some((p, t)) => (p, Some(t.to_string())),
            None => (input, None),
        };
        // Plain-HTTP artifact hosts (plans/0014 #1a): a non-GitHub URL
        // names the platform tarball itself; the mandatory `.sha256`
        // sidecar rides at `<url>.sha256`. The integrity model is
        // host-agnostic — the github.com restriction was v1 scope,
        // not a trust boundary.
        let github = path
            .strip_prefix("https://github.com/")
            .or_else(|| path.strip_prefix("http://github.com/"));
        if github.is_none() && (path.starts_with("https://") || path.starts_with("http://")) {
            return parse_tarball_url(path, tag);
        }
        let repo = if let Some(rest) = github {
            rest.trim_end_matches('/')
                .trim_end_matches(".git")
                .to_string()
        } else {
            path.to_string()
        };
        let has_slash = repo.contains('/');
        let short = if has_slash {
            let stem = repo.rsplit('/').next().unwrap_or(&repo).to_string();
            stem.strip_prefix("rootle-").unwrap_or(&stem).to_string()
        } else {
            repo.clone()
        };
        let repo = if has_slash {
            repo
        } else {
            // Bare name → the rootle-<name> convention (gh's gh- rule).
            format!("rootledev/rootle-{repo}")
        };
        let Some((owner, name)) = repo.split_once('/') else {
            return Err(ManagerError::User(format!(
                "provider reference {input:?} must be owner/repo, a GitHub URL, or a bare name"
            )));
        };
        if owner.is_empty() || name.is_empty() {
            return Err(ManagerError::User(format!("malformed reference {input:?}")));
        }
        Ok(Ref {
            repo,
            name: short,
            tag,
            tarball: None,
        })
    }
}

/// A plain-HTTP install reference: the URL names the platform tarball
/// (`rootle-<name>[-vX.Y.Z]-<target>.tar.gz`); name and tag derive
/// from the filename, an explicit `@tag` wins. The tag is receipt
/// provenance only — plain-HTTP installs are install-and-pin, never
/// tracked by `update`/`upgrade`.
fn parse_tarball_url(url: &str, tag: Option<String>) -> Result<Ref> {
    let file = url.rsplit('/').next().unwrap_or_default();
    let target = super::release::platform_target();
    let suffix = format!("-{target}.tar.gz");
    let stem = file.strip_suffix(&suffix).ok_or_else(|| {
        ManagerError::User(format!(
            "plain-HTTP install needs the platform tarball: a URL ending in {suffix}"
        ))
    })?;
    let stem = stem.strip_prefix("rootle-").unwrap_or(stem);
    let (name, derived) = match stem.rsplit_once('-') {
        Some((n, v)) if is_versionish(v) => (n, Some(normalize_tag(v))),
        _ => (stem, None),
    };
    if name.is_empty() {
        return Err(ManagerError::User(format!(
            "cannot derive a provider name from {url:?}"
        )));
    }
    Ok(Ref {
        repo: url.to_string(),
        name: name.to_string(),
        tag: tag.or(derived),
        tarball: Some(url.to_string()),
    })
}

/// `0.1.0` / `v0.1.0`-shaped filename tails.
fn is_versionish(s: &str) -> bool {
    let s = s.strip_prefix('v').unwrap_or(s);
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// Release tags are v-prefixed; keep the receipt consistent.
fn normalize_tag(v: &str) -> String {
    if v.starts_with('v') {
        v.to_string()
    } else {
        format!("v{v}")
    }
}

/// The binary name inside a release tarball (`rootle-<name>`).
pub(super) fn binary_name_of(r: &Ref) -> String {
    format!("rootle-{}", r.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_grammar() {
        // Bare name → the convention repo.
        assert_eq!(
            Ref::parse("gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None,
                tarball: None
            }
        );
        // owner/repo keeps its name; prefix stripped for the short name.
        assert_eq!(
            Ref::parse("rootledev/rootle-gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None,
                tarball: None
            }
        );
        // Full URL.
        assert_eq!(
            Ref::parse("https://github.com/rootledev/rootle-gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None,
                tarball: None
            }
        );
        // Tag pin.
        assert_eq!(
            Ref::parse("rootledev/rootle-gitlab@v0.1.0").unwrap().tag,
            Some("v0.1.0".into())
        );
        // Bare name with tag.
        assert_eq!(
            Ref::parse("gitlab@v0.2.0").unwrap().tag,
            Some("v0.2.0".into())
        );
        // Malformed.
        assert!(Ref::parse("").is_err());
        assert!(Ref::parse("justtext/").is_err());
        // Unprefixed repo: stem is the name.
        assert_eq!(Ref::parse("someone/myprovider").unwrap().name, "myprovider");
    }

    #[test]
    fn plain_http_tarball_urls() {
        let target = super::super::release::platform_target();
        let url =
            format!("https://artifacts.corp.example/providers/rootle-gitlab-0.1.0-{target}.tar.gz");
        let r = Ref::parse(&url).unwrap();
        assert_eq!(r.name, "gitlab");
        assert_eq!(r.tag, Some("v0.1.0".into()));
        assert_eq!(r.tarball, Some(url.clone()));
        // The URL is kept whole as the source (receipt provenance).
        assert_eq!(r.repo, url);

        // Versionless filename: no derived tag.
        let bare = format!("https://artifacts.corp.example/p/rootle-gitlab-{target}.tar.gz");
        let r = Ref::parse(&bare).unwrap();
        assert_eq!((r.name.as_str(), r.tag), ("gitlab", None));

        // An explicit @tag wins over the filename.
        let pinned = Ref::parse(&format!("{url}@v9.9.9")).unwrap();
        assert_eq!(pinned.tag, Some("v9.9.9".into()));

        // Wrong platform / not a tarball: a crisp user error.
        assert!(Ref::parse("https://artifacts.corp.example/rootle-gitlab.tar.gz").is_err());
        assert!(Ref::parse("https://artifacts.corp.example/rootle-gitlab").is_err());
    }
}
