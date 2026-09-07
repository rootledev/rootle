//! Content-addressable disk cache for the GitHub provider, under
//! ~/.cache/rootle/providers/github (plans/0005: each provider owns its
//! cache subtree; the TUI-level edit/ scratch stays at ~/.cache/rootle).
//! Trees are immutable (sha-keyed) — never invalidated, only evicted.
//! Ref mappings (owner/repo/branch → tree_sha + etag) are mutable and
//! revalidated with If-None-Match on every open.

use crate::types::TreeResponse;
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
/// (`~/.cache/rootle/providers/<name>`) — see doc/provider-protocol.md.
#[cfg(test)]
pub fn root() -> Option<PathBuf> {
    rootle_provider::paths::cache_dir().map(|d| d.join("rootle").join("providers").join("github"))
}

/// One-time move from the pre-provider layout (~/.cache/rootle/{trees,
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
    let base = rootle_provider::paths::cache_dir().map(|d| d.join("rootle"))?;
    migrate_from_legacy(&base);
    Some(base.join("providers").join("github"))
}

/// Path components (owner, repo, branch, sha) come from API responses
/// and are not trusted to be well-formed: percent-encode everything
/// outside [A-Za-z0-9_-] so a `feature/foo` branch (a legitimate
/// name) stays one path segment and separators / `..` can never
/// become path structure. Dots encode too — a literal `..` must not
/// survive as a component.
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Inverse of `encode_component`; invalid sequences pass through —
/// they can only come from files this module wrote.
fn decode_component(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| {
                (c as char)
                    .is_ascii_hexdigit()
                    .then(|| (c as char).to_digit(16).unwrap() as u8)
            };
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn ref_path(root: &std::path::Path, owner: &str, repo: &str, branch: &str) -> PathBuf {
    root.join("index/refs")
        .join(encode_component(owner))
        .join(encode_component(repo))
        .join(encode_component(branch))
}

fn tree_path(root: &std::path::Path, sha: &str) -> PathBuf {
    root.join("trees")
        .join(format!("{}.json", encode_component(sha)))
}

/// Blobs fan out by the sha's first 2 chars: blobs/<ab>/<rest>. The
/// encoded sha is pure ASCII, so the byte slices can't split a char.
fn blob_path(root: &std::path::Path, sha: &str) -> PathBuf {
    let sha = encode_component(sha);
    let split = 2.min(sha.len());
    root.join("blobs").join(&sha[..split]).join(&sha[split..])
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

pub fn cached_branch(owner: &str, repo: &str) -> Option<String> {
    let dir = root_or_migrate()?
        .join("index/refs")
        .join(encode_component(owner))
        .join(encode_component(repo));
    // First cached ref — skips any .tmp left by an interrupted write.
    // Entry names are encoded (a `feature/foo` branch is one entry);
    // decode back to the real branch name.
    let entry = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| !e.file_name().to_string_lossy().ends_with(".tmp"))?;
    Some(decode_component(&entry.file_name().into_string().ok()?))
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
        if let Ok(text) = std::fs::read_to_string(&file)
            && let Ok(entry) = serde_json::from_str::<RefCache>(&text)
        {
            referenced_trees.insert(entry.tree_sha);
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
        if let Ok(text) = std::fs::read_to_string(file)
            && let Ok(tree) = serde_json::from_str::<TreeResponse>(&text)
        {
            referenced_blobs.extend(
                tree.tree
                    .iter()
                    .filter(|e| e.kind == "blob")
                    .map(|e| e.sha.clone()),
            );
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
mod tests;
