use super::*;

fn commit(index: usize) -> HistoryCommit {
    HistoryCommit {
        sha: format!("revision-{index}"),
        commit: HistoryMetadata {
            message: format!("commit {index}\nbody"),
            author: HistoryAuthor {
                name: "author".into(),
                date: "2026-11-12".into(),
            },
        },
    }
}

#[test]
fn history_pages_past_ninety_nine_and_pins_the_original_revision() {
    for total in [100usize, 245, 500, 501] {
        let mut received = Vec::new();
        let mut calls = 0;
        let truncated = stream_history(
            reqwest::Url::parse("https://example.test/repos/o/r/commits").unwrap(),
            Some("src/文字.cpp"),
            Some("moving-branch"),
            500,
            &mut |entries| received.extend(entries),
            |url| {
                calls += 1;
                let url = reqwest::Url::parse(url).unwrap();
                let query: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                assert_eq!(query["per_page"], "100");
                assert_eq!(query["path"], "src/文字.cpp");
                assert_eq!(
                    query["sha"],
                    if calls == 1 {
                        "moving-branch"
                    } else {
                        "revision-0"
                    }
                );
                let start = (calls - 1) * 100;
                let end = (start + 100).min(total);
                Ok(Page {
                    body: (start..end).map(commit).collect(),
                    has_next: end < total,
                })
            },
        )
        .unwrap();
        assert_eq!(
            received
                .iter()
                .map(|entry| entry.sha.as_str())
                .collect::<Vec<_>>(),
            (0..total.min(500))
                .map(|index| format!("revision-{index}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(truncated, total > 500);
        assert_eq!(calls, total.min(500).div_ceil(100));
    }
}

#[test]
fn empty_or_repeated_pages_fail_without_discarding_delivered_history() {
    for repeat in [false, true] {
        let mut delivered = Vec::new();
        let mut calls = 0;
        let result = stream_history(
            reqwest::Url::parse("https://example.test/commits").unwrap(),
            None,
            None,
            500,
            &mut |entries| delivered.extend(entries),
            |_| {
                calls += 1;
                Ok(Page {
                    body: if calls == 1 || repeat {
                        vec![commit(0)]
                    } else {
                        Vec::new()
                    },
                    has_next: true,
                })
            },
        );
        assert!(result.is_err());
        assert_eq!(delivered[0].sha, "revision-0");
        assert_eq!(delivered.len(), 1);
        assert_eq!(calls, 2);
    }
}

#[test]
fn zero_limit_probes_without_emitting_a_commit() {
    let mut received = Vec::new();
    let truncated = stream_history(
        reqwest::Url::parse("https://example.test/commits").unwrap(),
        None,
        None,
        0,
        &mut |entries| received.extend(entries),
        |_| {
            Ok(Page {
                body: vec![commit(0)],
                has_next: false,
            })
        },
    )
    .unwrap();
    assert!(truncated);
    assert!(received.is_empty());
}
