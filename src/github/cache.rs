//! Content-addressable disk cache for the GitHub provider, under
//! ~/.cache/ghx/providers/github (plans/0005: each provider owns its
//! cache subtree; the TUI-level edit/ scratch stays at ~/.cache/ghx).
//! Trees are immutable (sha-keyed) — never invalidated, only evicted.
//! Ref mappings (owner/repo/branch → tree_sha + etag) are mutable and
//! revalidated with If-None-Match on every open.

use crate::github::types::TreeResponse;
use serde::{Deserialize, Serialize};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefCache {
    pub tree_sha: String,
    pub etag: Option<String>,
}

/// The provider's cache subtree. Other providers must use their own
/// (`~/.cache/ghx/providers/<name>`) — see doc/provider-protocol.md.
#[cfg(test)]
pub fn root() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("ghx").join("providers").join("github"))
}

/// One-time move from the pre-provider layout (~/.cache/ghx/{trees,
/// blobs,index}) into this provider's subtree. Best-effort: partial
/// moves just mean re-fetches.
fn migrate_from_legacy(base: &Path) {
    let mine = base.join("providers").join("github");
    for dir in ["trees", "blobs", "index"] {
        let old = base.join(dir);
        let new = mine.join(dir);
        if old.exists() && !new.exists() {
            let _ = std::fs::create_dir_all(&mine);
            let _ = std::fs::rename(&old, &new);
        }
    }
}

/// Resolve the cache root, migrating the legacy layout once.
fn root_or_migrate() -> Option<PathBuf> {
    let base = dirs::cache_dir().map(|d| d.join("ghx"))?;
    migrate_from_legacy(&base);
    Some(base.join("providers").join("github"))
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
    let path = blob_path(&root_or_migrate()?, sha);
    let bytes = std::fs::read(&path).ok()?;
    touch(&path); // mtime = last-used, drives LRU eviction
    Some(bytes)
}

pub fn write_blob(sha: &str, bytes: &[u8]) -> io::Result<()> {
    let Some(root) = root_or_migrate() else {
        return Ok(());
    };
    atomic_write(&blob_path(&root, sha), bytes)
}

/// The branch a repo was last opened on (first cached ref) — lets
/// fetch_tree skip the repo-meta round trip entirely.
pub fn cached_branch(owner: &str, repo: &str) -> Option<String> {
    let dir = root_or_migrate()?.join("index/refs").join(owner).join(repo);
    let entry = std::fs::read_dir(dir).ok()?.next()?.ok()?;
    entry.file_name().into_string().ok()
}

pub fn read_ref(owner: &str, repo: &str, branch: &str) -> Option<RefCache> {
    let path = ref_path(&root_or_migrate()?, owner, repo, branch);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_ref(owner: &str, repo: &str, branch: &str, entry: &RefCache) -> io::Result<()> {
    let Some(root) = root_or_migrate() else {
        return Ok(());
    };
    atomic_write(
        &ref_path(&root, owner, repo, branch),
        serde_json::to_string(entry)?.as_bytes(),
    )
}

pub fn read_tree(sha: &str) -> Option<TreeResponse> {
    let path = tree_path(&root_or_migrate()?, sha);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_tree(tree: &TreeResponse) -> io::Result<()> {
    let Some(root) = root_or_migrate() else {
        return Ok(());
    };
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

// ---------------------------------------------------------------------------
// Hardening (PLAN.md §8 / milestone 7)
//
// LRU: file mtime is last-used — `read_blob` touches it, so eviction
// needs no index file. Sweep: a tree not referenced by any cached ref,
// and a blob not referenced by any cached tree, are orphans.
// ---------------------------------------------------------------------------

/// Startup hardening: sweep orphans, then evict least-recently-used
/// blobs while total size exceeds `max_bytes`.
pub fn harden(max_bytes: u64) {
    let Some(root) = root_or_migrate() else {
        return;
    };
    sweep_orphans(&root);
    evict_blobs(&root, max_bytes);
}

/// Best-effort mtime bump so eviction sees the blob as recently used.
fn touch(path: &PathBuf) {
    let _ = filetime::set_file_mtime(path, filetime::FileTime::now());
}

fn walk_files(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn sweep_orphans(root: &Path) {
    let index = root.join("index").join("refs");
    let trees_dir = root.join("trees");
    let blobs_dir = root.join("blobs");

    // Tree shas referenced by at least one cached ref.
    let mut referenced_trees = std::collections::HashSet::new();
    let mut ref_files = Vec::new();
    walk_files(&index, &mut ref_files);
    for file in ref_files {
        if let Ok(text) = std::fs::read_to_string(&file) {
            if let Ok(entry) = serde_json::from_str::<RefCache>(&text) {
                referenced_trees.insert(entry.tree_sha);
            }
        }
    }

    // Delete unreferenced trees; keep the referenced ones' filenames.
    let mut live_trees = Vec::new();
    let mut tree_files = Vec::new();
    walk_files(&trees_dir, &mut tree_files);
    for file in tree_files {
        let sha = file
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if referenced_trees.contains(&sha) {
            live_trees.push(file);
        } else {
            let _ = std::fs::remove_file(&file);
        }
    }

    // Blob shas referenced by a live tree.
    let mut referenced_blobs = std::collections::HashSet::new();
    for file in &live_trees {
        if let Ok(text) = std::fs::read_to_string(file) {
            if let Ok(tree) = serde_json::from_str::<TreeResponse>(&text) {
                referenced_blobs.extend(
                    tree.tree
                        .iter()
                        .filter(|e| e.kind == "blob")
                        .map(|e| e.sha.clone()),
                );
            }
        }
    }

    // Delete unreferenced blobs (sha = <fanout>/<rest>).
    let mut blob_files = Vec::new();
    walk_files(&blobs_dir, &mut blob_files);
    for file in blob_files {
        let sha = format!(
            "{}{}",
            file.parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy(),
            file.file_name().unwrap_or_default().to_string_lossy()
        );
        if !referenced_blobs.contains(&sha) {
            let _ = std::fs::remove_file(&file);
        }
    }
}

fn evict_blobs(root: &Path, max_bytes: u64) {
    let blobs_dir = root.join("blobs");
    let mut files = Vec::new();
    walk_files(&blobs_dir, &mut files);

    let mut entries: Vec<(PathBuf, u64, i64)> = files
        .into_iter()
        .filter_map(|f| {
            let meta = std::fs::metadata(&f).ok()?;
            Some((f, meta.len(), meta.mtime()))
        })
        .collect();
    let mut total: u64 = entries.iter().map(|(_, size, _)| size).sum();
    if total <= max_bytes {
        return;
    }

    // Oldest (least-recently-used) first.
    entries.sort_by_key(|(_, _, mtime)| *mtime);
    for (path, size, _) in entries {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ghx-cache-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put_blob(root: &Path, sha: &str, size: usize, mtime: i64) {
        let path = blob_path(root, sha);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![0u8; size]).unwrap();
        filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(mtime, 0)).unwrap();
    }

    fn put_tree(root: &Path, sha: &str, blob_shas: &[&str]) {
        let tree = TreeResponse {
            sha: sha.into(),
            truncated: false,
            tree: blob_shas
                .iter()
                .map(|s| crate::github::types::TreeEntry {
                    path: format!("file-{}", &s[..4]),
                    kind: "blob".into(),
                    sha: s.to_string(),
                    size: Some(10),
                })
                .collect(),
        };
        let path = tree_path(root, sha);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(&tree).unwrap()).unwrap();
    }

    fn put_ref(root: &Path, tree_sha: &str) {
        let path = ref_path(root, "o", "r", "main");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let entry = RefCache {
            tree_sha: tree_sha.into(),
            etag: None,
        };
        std::fs::write(&path, serde_json::to_string(&entry).unwrap()).unwrap();
    }

    #[test]
    fn eviction_removes_oldest_blobs_first() {
        let root = temp_root("evict");
        put_blob(&root, "aa1234", 100, 1000); // oldest
        put_blob(&root, "bb1234", 100, 2000);
        put_blob(&root, "cc1234", 100, 3000); // newest
        // Cap at 250 bytes: aa must go, bb and cc stay.
        evict_blobs(&root, 250);
        assert!(!blob_path(&root, "aa1234").exists(), "oldest evicted");
        assert!(blob_path(&root, "bb1234").exists());
        assert!(blob_path(&root, "cc1234").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sweep_removes_orphan_trees_and_blobs() {
        let root = temp_root("sweep");
        put_ref(&root, "livetree");
        put_tree(&root, "livetree", &["bbbb11"]); // referenced tree + blob
        put_tree(&root, "deadt1", &["cccc22"]); // orphan tree
        put_blob(&root, "bbbb11", 10, 1000); // referenced by live tree
        put_blob(&root, "dddd33", 10, 1000); // unreferenced blob

        sweep_orphans(&root);

        assert!(tree_path(&root, "livetree").exists());
        assert!(!tree_path(&root, "deadt1").exists(), "orphan tree swept");
        assert!(blob_path(&root, "bbbb11").exists(), "live blob kept");
        assert!(!blob_path(&root, "dddd33").exists(), "orphan blob swept");
        let _ = std::fs::remove_dir_all(&root);
    }

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

    #[test]
    fn cached_branch_roundtrip() {
        if root().is_none() {
            return;
        }
        // write_ref lands in index/refs/owner/repo/branch; cached_branch
        // must find it back without any network.
        write_ref(
            "ratatui",
            "ratatui",
            "main",
            &RefCache {
                tree_sha: "abc".into(),
                etag: None,
            },
        )
        .unwrap();
        assert_eq!(cached_branch("ratatui", "ratatui").as_deref(), Some("main"));
        assert!(cached_branch("ratatui", "never-opened").is_none());
        if let Some(root) = root() {
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
