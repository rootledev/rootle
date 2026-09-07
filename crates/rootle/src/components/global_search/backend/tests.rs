use super::*;

fn needles(q: &str) -> Vec<String> {
    q.split_whitespace().map(str::to_string).collect()
}

/// GitHub-"go to file" semantics (verified against github.com):
/// fuzzy subsequence over the full path, directory names included,
/// filename matches ranked above path matches.
#[test]
fn file_find_matches_like_github_go_to_file() {
    // The canonical example: urldef → djangosite/urls/default.py
    // (url in a directory, def in the file name).
    let s = file_find_score("djangosite/urls/default.py", &needles("urldef"));
    assert!(s.is_some(), "subsequence across dir + file must match");

    // Directory names match like file names (substring).
    assert!(file_find_score("something/term/cargo.toml", &needles("term")).is_some());

    // Space-separated fragments each must match, in any spread.
    assert!(file_find_score("djangosite/urls/default.py", &needles("url def")).is_some());
    assert!(file_find_score("djangosite/urls/default.py", &needles("url zzz")).is_none());

    // In-order only: reversed chars never match.
    assert!(file_find_score("src/main.rs", &needles("msni")).is_none());
    assert!(file_find_score("src/main.rs", &needles("mrs")).is_some());

    // Empty query matches everything at 0.
    assert_eq!(file_find_score("src/main.rs", &[]), Some(0));
}

#[test]
fn file_find_ranks_filename_above_path_above_scattered() {
    let starts = file_find_score("src/terminal.rs", &needles("term")).unwrap();
    let inside = file_find_score("src/myterm.rs", &needles("term")).unwrap();
    let dir = file_find_score("src/term/cargo.toml", &needles("term")).unwrap();
    let scattered = file_find_score("src/test/remote.rs", &needles("term")).unwrap();
    assert!(
        starts > inside,
        "needle starting the file name beats one inside it"
    );
    assert!(inside > dir, "file-name match beats a directory-only match");
    assert!(
        dir > scattered,
        "contiguous path match beats scattered chars"
    );
}

#[test]
fn locate_in_blob_folds_regions_and_counts() {
    let text = b"line one\nmatch here\nbetween\nanother match\ntail";
    let needles = vec!["match".to_string()];
    let (line, preview, count) = locate_in_blob(text, &needles).unwrap();
    assert_eq!(line, 2);
    assert_eq!(count, 2);
    // One merged region: the two matches sit 2 lines apart with
    // one context line each side, so everything folds together.
    let nos: Vec<u32> = preview.iter().map(|(n, _)| *n).collect();
    assert_eq!(nos, vec![1, 2, 3, 4, 5]);
    // Binary blobs never locate.
    assert!(locate_in_blob(b"\x00\x01match", &needles).is_none());
}

#[test]
fn code_query_maps_scope_and_extension() {
    use super::SearchKind;
    // Wire surface (plans/0008 §4): these strings reach external
    // providers verbatim.
    assert_eq!(
        code_query(SearchKind::Grep, "needle", "global", ""),
        "needle"
    );
    assert_eq!(
        code_query(SearchKind::FileFind, "main", "repo:o/r", ".rs"),
        "path:main repo:o/r extension:rs"
    );
    assert_eq!(
        code_query(SearchKind::Grep, "q", "org:x", "rs"),
        "q org:x extension:rs"
    );
}

/// The index can lie by omission (young repos aren't in GitHub's
/// code index): a scoped grep that the API answers with a silent
/// zero falls back to grepping the tarball locally — GitHub AND
/// semantics, negation, real line numbers, git blob shas.
#[test]
fn scoped_grep_falls_back_to_tarball_on_silent_zero() {
    use rootle_provider::{Capabilities, Provider, ProviderResult, SearchCodeResult, TreeResult};

    let a_rs = b"fn target() {}\n";
    let b_rs = b"nothing relevant here\n";
    let c_rs = b"target\nand target again\n";
    let bin = vec![0u8, 159, 146, 150, 0, 1, 2, 3];

    // codeload shape: every path under "owner-repo-sha/".
    let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(enc);
    let mut add = |path: &str, bytes: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("o-r-deadbeef/{path}"), bytes)
            .unwrap();
    };
    add("src/a.rs", a_rs);
    add("src/b.rs", b_rs);
    add("src/c.rs", c_rs);
    add("img.bin", &bin);
    let tarball = builder.into_inner().unwrap().finish().unwrap();

    struct Mock(Vec<u8>);
    impl Provider for Mock {
        fn name(&self) -> &str {
            "mock"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                commit: false,
                orgs: false,
                code_search: true,
                file_search: true,
                refs: false,
                log: false,
                blame: false,
            }
        }
        fn search(&self, _: &str) -> ProviderResult<Vec<rootle_provider::SearchItem>> {
            Err("mock".into())
        }
        fn org_repos(&self, _: &str) -> ProviderResult<Vec<rootle_provider::RepoInfo>> {
            Err("mock".into())
        }
        fn fetch_tree(
            &self,
            _: &rootle_provider::RepoId,
            _: Option<&rootle_provider::GitRef>,
        ) -> ProviderResult<TreeResult> {
            Ok(TreeResult {
                entries: Vec::new(),
                truncated: false,
                branch: "main".into(),
            })
        }
        fn fetch_blob(
            &self,
            _: &rootle_provider::RepoId,
            _: &rootle_provider::Sha,
        ) -> ProviderResult<Vec<u8>> {
            Err("mock".into())
        }
        fn search_code(&self, _: &str) -> ProviderResult<SearchCodeResult> {
            // The silent zero: total index omission, no error.
            Ok(SearchCodeResult {
                hits: Vec::new(),
                truncated: false,
                index_as_of: None,
            })
        }
        fn clone_url(&self, _: &rootle_provider::RepoId) -> ProviderResult<String> {
            Err("mock".into())
        }
        fn web_url(
            &self,
            _: &rootle_provider::RepoId,
            _: &str,
            _: Option<&rootle_provider::GitRef>,
            _: Option<u32>,
            _: Option<u32>,
            _: bool,
        ) -> ProviderResult<String> {
            Err("mock".into())
        }
        fn org_url(&self, _: &str) -> ProviderResult<String> {
            Err("mock".into())
        }
        fn source_tarball(&self, _: &rootle_provider::RepoId) -> ProviderResult<Vec<u8>> {
            Ok(self.0.clone())
        }
    }

    let provider = Mock(tarball);
    let run = |query: &str| -> Vec<RawHit> {
        let hits = std::sync::Mutex::new(Vec::new());
        run_view_search(
            &provider,
            SearchKind::Grep,
            query,
            "repo:o/r",
            "",
            &|batch: Vec<RawHit>| hits.lock().unwrap().extend(batch),
        )
        .unwrap();
        hits.into_inner().unwrap()
    };

    let hits = run("target");
    let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["src/a.rs", "src/c.rs"],
        "binary skipped, non-matches skipped"
    );
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].match_count, 1, "a.rs has one matching line");
    assert_eq!(hits[1].match_count, 2, "c.rs has two");
    assert_eq!(hits[0].branch, "main");
    // git blob sha — what fetch_blob/yank/edit address.
    let want = {
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(format!("blob {}\0", a_rs.len()));
        h.update(a_rs);
        h.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(hits[0].sha, want);
    assert!(!hits[0].stale);
    assert!(
        hits[0].preview.iter().any(|(_, l)| l.contains("target")),
        "preview carries the matched line"
    );

    // GitHub AND semantics: both terms must occur in the file.
    let hits = run("target again");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/c.rs");

    // Negation subtracts.
    let hits = run("target -again");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/a.rs");
}
