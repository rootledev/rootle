//! Recursive trees, conditional ref revalidation and cache healing.

use super::transport::Conditional;
use super::{API, GitHubClient};
use crate::types::{RepoMeta, TreeResponse};
use rootle_provider::{ErrorKind, ProviderError, ProviderResult};

impl GitHubClient {
    /// Fetch a repo's full recursive tree with the sha-keyed cache
    /// (PLAN.md §8): revalidate the branch ref with If-None-Match
    /// (304 = free, tree unchanged), cache tree bodies by their sha.
    /// Also returns the default branch (URL building, yank). v1.5:
    /// `ref_` pins another branch/tag/sha — the tree endpoint takes
    /// any ref, and a sha's tree is immutable, so the same etag
    /// revalidation is correct there.
    pub fn fetch_tree(
        &self,
        owner: &str,
        repo: &str,
        ref_: Option<&str>,
    ) -> ProviderResult<(
        TreeResponse,
        /*truncated*/ bool,
        /*branch*/ String,
    )> {
        if let Some(r) = ref_ {
            return self.fetch_tree_on(owner, repo, r);
        }
        // Cache-first branch resolution: a repo we've opened before
        // costs zero extra calls here (no GET /repos/{o}/{r}).
        let cached_branch = crate::cache::cached_branch(owner, repo);
        let branch = match &cached_branch {
            Some(b) => {
                rootle_provider::trace(&format!("tree branch cached {owner}/{repo} {b}"));
                b.clone()
            }
            None => {
                rootle_provider::trace(&format!("tree branch meta-fetch {owner}/{repo}"));
                let meta: RepoMeta = self.get(&format!("{API}/repos/{owner}/{repo}"))?;
                meta.default_branch
            }
        };
        match self.fetch_tree_on(owner, repo, &branch) {
            // The default branch was renamed since we cached it:
            // resolve fresh and try once more.
            Err(e) if cached_branch.is_some() && e.kind == ErrorKind::NotFound => {
                let meta: RepoMeta = self.get(&format!("{API}/repos/{owner}/{repo}"))?;
                self.fetch_tree_on(owner, repo, &meta.default_branch)
            }
            other => other,
        }
    }

    fn fetch_tree_on(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> ProviderResult<(
        TreeResponse,
        /*truncated*/ bool,
        /*branch*/ String,
    )> {
        let cached_ref = crate::cache::read_ref(owner, repo, branch);
        let url = format!("{API}/repos/{owner}/{repo}/git/trees/{branch}?recursive=1");

        let etag = cached_ref.as_ref().and_then(|r| r.etag.clone());
        match self.get_conditional::<TreeResponse>(&url, etag.as_deref())? {
            Conditional::NotModified => {
                let sha = cached_ref.expect("304 without a cached ref").tree_sha;
                match crate::cache::read_tree(&sha) {
                    Some(tree) => Ok((tree.clone(), tree.truncated, branch.to_string())),
                    None => {
                        // A cache read that cannot be satisfied is a
                        // miss, not an error. The startup orphan sweep
                        // can race a fetch's tree-then-ref write order
                        // and delete a tree its ref already points at;
                        // the etag then 304s forever against a missing
                        // body (sticky unopenable repo). Refetch
                        // unconditionally — the cache is only an
                        // optimization, and this re-stores the tree
                        // and ref, healing both.
                        rootle_provider::trace(&format!(
                            "304 but tree {sha} missing from cache; refetching"
                        ));
                        let Conditional::Fresh { body, etag } =
                            self.get_conditional::<TreeResponse>(&url, None)?
                        else {
                            return Err(ProviderError::other(
                                "unconditional revalidation returned 304",
                            ));
                        };
                        self.store_tree(owner, repo, branch, body, etag)
                    }
                }
            }
            Conditional::Fresh { body, etag } => self.store_tree(owner, repo, branch, body, etag),
        }
    }

    /// Persist a fetched tree + its ref (tree first — durable before
    /// discoverable; the sweep race above is absorbed by the miss
    /// fallback).
    fn store_tree(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        body: TreeResponse,
        etag: Option<String>,
    ) -> ProviderResult<(
        TreeResponse,
        /*truncated*/ bool,
        /*branch*/ String,
    )> {
        crate::cache::write_tree(&body).map_err(|e| ProviderError::other(e.to_string()))?;
        crate::cache::write_ref(
            owner,
            repo,
            branch,
            &crate::cache::RefCache {
                tree_sha: body.sha.clone(),
                etag,
            },
        )
        .map_err(|e| ProviderError::other(e.to_string()))?;
        let truncated = body.truncated;
        Ok((body, truncated, branch.to_string()))
    }
}
