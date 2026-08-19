//! Content-addressable disk cache under ~/.cache/ghx (PLAN.md §8).
//! Trees are immutable (sha-keyed) — never invalidated, only evicted.
//! Ref mappings (owner/repo/branch → tree_sha + etag) are mutable and
//! revalidated with If-None-Match on every open.

use crate::github::types::TreeResponse;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefCache {
    pub tree_sha: String,
    pub etag: Option<String>,
}

pub fn root() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("ghx"))
}

fn ref_path(root: &std::path::Path, owner: &str, repo: &str, branch: &str) -> PathBuf {
    root.join("index/refs").join(owner).join(repo).join(branch)
}

fn tree_path(root: &std::path::Path, sha: &str) -> PathBuf {
    root.join("trees").join(format!("{sha}.json"))
}

/// Blobs fan out by the sha's first 2 chars: blobs/<ab>/<rest>.
fn blob_path(root: &std::path::Path, sha: &str) -> PathBuf {
    root.join("blobs")
        .join(&sha[..2.min(sha.len())])
        .join(&sha[2.min(sha.len())..])
}

pub fn read_blob(sha: &str) -> Option<Vec<u8>> {
    let path = blob_path(&root()?, sha);
    std::fs::read(path).ok()
}

pub fn write_blob(sha: &str, bytes: &[u8]) -> io::Result<()> {
    let Some(root) = root() else { return Ok(()) };
    atomic_write(&blob_path(&root, sha), bytes)
}

pub fn read_ref(owner: &str, repo: &str, branch: &str) -> Option<RefCache> {
    let path = ref_path(&root()?, owner, repo, branch);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_ref(owner: &str, repo: &str, branch: &str, entry: &RefCache) -> io::Result<()> {
    let Some(root) = root() else { return Ok(()) };
    atomic_write(
        &ref_path(&root, owner, repo, branch),
        serde_json::to_string(entry)?.as_bytes(),
    )
}

pub fn read_tree(sha: &str) -> Option<TreeResponse> {
    let path = tree_path(&root()?, sha);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_tree(tree: &TreeResponse) -> io::Result<()> {
    let Some(root) = root() else { return Ok(()) };
    atomic_write(
        &tree_path(&root, &tree.sha),
        serde_json::to_string(tree)?.as_bytes(),
    )
}

/// tmp + rename — a kill mid-write never yields a corrupt entry.
fn atomic_write(path: &PathBuf, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_round_trip_by_sha() {
        // Uses the real cache root; harmless, small, and overwritten by
        // design. Skipped silently when no cache dir exists.
        if root().is_none() {
            return;
        }
        let tree = TreeResponse {
            sha: "test-sha-ghx".into(),
            truncated: false,
            tree: vec![],
        };
        write_tree(&tree).unwrap();
        let loaded = read_tree("test-sha-ghx").unwrap();
        assert_eq!(loaded.sha, "test-sha-ghx");
    }
}
