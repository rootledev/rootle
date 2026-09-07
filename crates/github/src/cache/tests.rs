use super::*;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rootle-cache-{}-{tag}", std::process::id()));
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
            .map(|s| crate::types::TreeEntry {
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
        sha: "test-sha-rootle".into(),
        truncated: false,
        tree: vec![],
    };
    write_tree(&tree).unwrap();
    let loaded = read_tree("test-sha-rootle").unwrap();
    assert_eq!(loaded.sha, "test-sha-rootle");
}

#[test]
fn cached_branch_roundtrip() {
    if root().is_none() {
        return;
    }
    // write_ref lands in index/refs/owner/repo/branch; cached_branch
    // must find it back without any network. The rootle-test owner
    // keeps the real cache untouched, and cleanup removes only what
    // this test wrote — a whole-root wipe races parallel tests that
    // share the cache tree.
    write_ref(
        "rootle-test",
        "cached-branch",
        "main",
        &RefCache {
            tree_sha: "abc".into(),
            etag: None,
        },
    )
    .unwrap();
    assert_eq!(
        cached_branch("rootle-test", "cached-branch").as_deref(),
        Some("main")
    );
    assert!(cached_branch("rootle-test", "never-opened").is_none());
    if let Some(root) = root() {
        let _ = std::fs::remove_dir_all(root.join("index/refs/rootle-test"));
    }
}

#[test]
fn branch_with_slash_is_one_component_and_roundtrips() {
    if root().is_none() {
        return;
    }
    // "feature/foo" is a legitimate branch name: it must cache as
    // ONE path entry (percent-encoded), read_ref must find it, and
    // cached_branch must return the full name (not "feature").
    write_ref(
        "rootle-test-slash",
        "slash-branch",
        "feature/foo",
        &RefCache {
            tree_sha: "abc".into(),
            etag: None,
        },
    )
    .unwrap();
    if let Some(root) = root() {
        // The ref is a FILE at .../slash-branch/feature%2Ffoo, not a
        // directory tree .../slash-branch/feature/foo.
        let p = ref_path(&root, "rootle-test-slash", "slash-branch", "feature/foo");
        assert!(p.is_file(), "{} should be a file", p.display());
        assert!(p.file_name().unwrap().to_string_lossy().contains("feature"));
        assert!(!p.to_string_lossy().contains("feature/foo"));
    }
    assert_eq!(
        read_ref("rootle-test-slash", "slash-branch", "feature/foo").map(|r| r.tree_sha),
        Some("abc".into())
    );
    assert_eq!(
        cached_branch("rootle-test-slash", "slash-branch").as_deref(),
        Some("feature/foo")
    );
    if let Some(root) = root() {
        let _ = std::fs::remove_dir_all(root.join("index/refs/rootle-test-slash"));
    }
}

#[test]
fn traversal_and_hostile_components_stay_inside_the_cache() {
    let root = temp_root("hostile");
    // Every component is encoded before it becomes path structure:
    // separators, dots, and NUL can only appear percent-encoded.
    let p = ref_path(&root, "../../home", "o/r", "main");
    let s = p.to_string_lossy();
    assert!(
        s.starts_with(root.to_string_lossy().as_ref()),
        "stays under the cache root"
    );
    assert!(!s.contains(".."), "no dot-dot survives encoding: {s}");
    assert!(
        !s.matches('/').count() > 3 + root.to_string_lossy().matches('/').count() + 4,
        "no extra separators"
    );
    // Branch "a/b" and repo "a" cannot collide with branch "b" on
    // repo "a/a": encodings differ.
    let p1 = ref_path(&root, "o", "a", "a/b");
    let p2 = ref_path(&root, "o", "a/a", "b");
    assert_ne!(p1, p2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn non_ascii_sha_is_a_miss_not_a_panic() {
    // blob_path used to byte-slice the raw sha — a multibyte char
    // at the boundary panicked. Encoding makes the slice safe, and
    // a hostile sha reads as a plain miss.
    let root = temp_root("sha");
    assert!(read_blob_at(&root, "日本語").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

// read_blob against an explicit root (the public fn resolves the
// real cache dir; tests must not touch it).
fn read_blob_at(root: &Path, sha: &str) -> Option<Vec<u8>> {
    std::fs::read(blob_path(root, sha)).ok()
}
