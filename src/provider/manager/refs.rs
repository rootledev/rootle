//! The install-reference grammar: what `rootle provider install`
//! accepts and how it resolves to repo, short name, and pin.

use super::{ManagerError, Result};

/// A resolved install reference (the grammar from the plan):
/// `gitlab` | `owner/repo` | `https://github.com/owner/repo`, each
/// optionally `@tag`-pinned.
#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    /// owner/repo
    pub repo: String,
    /// The bare name (repo stem minus the rootle- prefix, or the stem
    /// itself when unprefixed).
    pub name: String,
    pub tag: Option<String>,
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
        let repo = if let Some(rest) = path
            .strip_prefix("https://github.com/")
            .or_else(|| path.strip_prefix("http://github.com/"))
        {
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
        })
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
                tag: None
            }
        );
        // owner/repo keeps its name; prefix stripped for the short name.
        assert_eq!(
            Ref::parse("rootledev/rootle-gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None
            }
        );
        // Full URL.
        assert_eq!(
            Ref::parse("https://github.com/rootledev/rootle-gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None
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
}
