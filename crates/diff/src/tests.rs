use super::*;

#[test]
fn replacement_emphasis_uses_each_sides_own_extent() {
    let diff = FileDiff::parse("@@ -1 +1 @@\n-let x = hone(a);\n+let x = hone(b, c);\n").unwrap();
    let hunk = &diff.hunks[0];
    let spans = hunk.changed_spans();
    assert_eq!(&hunk.lines[0].text[spans[0].as_ref().unwrap().range()], "a");
    assert_eq!(
        &hunk.lines[1].text[spans[1].as_ref().unwrap().range()],
        "b, c"
    );
    let unicode = FileDiff::parse("@@ -1 +1 @@\n-文字(短)\n+文字(広い範囲)\n").unwrap();
    let spans = unicode.hunks[0].changed_spans();
    assert_eq!(
        &unicode.hunks[0].lines[1].text[spans[1].as_ref().unwrap().range()],
        "広い範囲"
    );
}

#[test]
fn empty_sides_never_emit_line_zero_and_counters_reset() {
    let diff =
        FileDiff::parse("@@ -0,0 +1,2 @@\n+a\n+b\n@@ -42 +44 @@ context\n-old\n+new\n").unwrap();
    assert!(
        diff.hunks[0]
            .lines
            .iter()
            .all(|line| line.old_line.is_none())
    );
    assert_eq!(diff.hunks[1].lines[0].old_line.unwrap().get(), 42);
    assert_eq!(diff.hunks[1].lines[1].new_line.unwrap().get(), 44);
    assert_eq!(diff.hunks[1].header(), "@@ -42,1 +44,1 @@");
    assert_eq!(diff.hunks[1].context, "context");
}

#[test]
fn patch_transport_terminator_does_not_change_source_eof() {
    let diff =
        FileDiff::parse("@@ -1 +1 @@\n-old\r\n\\ No newline at end of file\n+new\r").unwrap();
    assert_eq!(diff.hunks[0].lines[0].text, "old\r");
    assert!(!diff.hunks[0].lines[0].has_newline);
    assert!(diff.hunks[0].lines[1].has_newline);
}

#[test]
fn incomplete_malformed_or_overflowing_hunks_are_honest_errors() {
    for patch in [
        "@@ -1,9 +1,9 @@\n context\n",
        "@@ -1 +1 @@\nunprefixed\n",
        "@@ -0 +1 @@\n-a\n+b\n",
        "@@ -4294967295,2 +1 @@\n-a\n-b\n+c\n",
    ] {
        assert!(FileDiff::parse(patch).is_err(), "{patch}");
    }
    let diff = FileDiff::parse("@@ -4294967295 +1 @@\n-a\n+b\n").unwrap();
    assert_eq!(diff.hunks[0].lines[0].old_line.unwrap().get(), u32::MAX);
}

#[test]
fn unpaired_lines_do_not_borrow_another_hunks_emphasis() {
    let diff = FileDiff::parse("@@ -1,2 +1 @@\n-one\n-two\n+three\n").unwrap();
    assert!(diff.hunks[0].changed_spans()[1].is_none());
}
