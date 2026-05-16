//! Unified-diff parser tests.
//!
//! Coverage matrix:
//!   - empty input
//!   - single-hunk, single-file, with removed + added + context lines
//!   - hunk header with a single-line range (`@@ -42 +42 @@`)
//!   - hunk header with trailing context after the second `@@`
//!   - multi-hunk, single-file
//!   - multi-file, multi-hunk
//!   - new-file (`--- /dev/null`)
//!   - "\\ No newline at end of file" sentinel ignored
//!   - removed_ranges merges adjacent runs and splits across context
//!   - property: removed_ranges()'s output is always sorted and
//!     non-overlapping and every reported line is a `-` line in the hunk

use reckon::diff::{parse, FileDiff, Hunk, HunkLine};

#[test]
fn parses_empty_input() {
    assert!(parse("").unwrap().is_empty());
}

#[test]
fn parses_simple_single_hunk() {
    let d = "--- a/foo.py
+++ b/foo.py
@@ -10,3 +10,2 @@ def f():
     a = 1
-    b = 2
     c = 3
";
    let files = parse(d).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].old_path, "foo.py");
    assert_eq!(files[0].new_path, "foo.py");
    assert_eq!(files[0].hunks.len(), 1);
    let h = &files[0].hunks[0];
    assert_eq!(h.old_start, 10);
    assert_eq!(h.old_count, 3);
    assert_eq!(h.new_start, 10);
    assert_eq!(h.new_count, 2);
    assert_eq!(h.removed_ranges(), vec![(11, 11)]);
}

#[test]
fn hunk_header_single_line_range_defaults_count_to_one() {
    let d = "--- a/foo.py
+++ b/foo.py
@@ -42 +42 @@
-old
+new
";
    let files = parse(d).unwrap();
    let h = &files[0].hunks[0];
    assert_eq!(h.old_start, 42);
    assert_eq!(h.old_count, 1);
    assert_eq!(h.new_count, 1);
    assert_eq!(h.removed_ranges(), vec![(42, 42)]);
}

#[test]
fn hunk_header_with_trailing_context_after_second_at_at() {
    let d = "--- a/foo.py
+++ b/foo.py
@@ -1,3 +1,3 @@ def retry_payment(order_id):
 a
-b
+c
 d
";
    let files = parse(d).unwrap();
    let h = &files[0].hunks[0];
    assert_eq!(h.old_start, 1);
    assert_eq!(h.removed_ranges(), vec![(2, 2)]);
}

#[test]
fn multi_hunk_single_file_keeps_each_hunk_distinct() {
    let d = "--- a/foo.py
+++ b/foo.py
@@ -1,2 +1,1 @@
-a
 b
@@ -10,2 +9,1 @@
-x
 y
";
    let files = parse(d).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].hunks.len(), 2);
    assert_eq!(files[0].hunks[0].removed_ranges(), vec![(1, 1)]);
    assert_eq!(files[0].hunks[1].removed_ranges(), vec![(10, 10)]);
}

#[test]
fn multi_file_multi_hunk() {
    let d = "--- a/foo.py
+++ b/foo.py
@@ -1,2 +1,1 @@
-a
 b
--- a/bar.py
+++ b/bar.py
@@ -5,3 +5,2 @@
 x
-y
 z
";
    let files = parse(d).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].old_path, "foo.py");
    assert_eq!(files[1].old_path, "bar.py");
    assert_eq!(files[0].hunks[0].removed_ranges(), vec![(1, 1)]);
    assert_eq!(files[1].hunks[0].removed_ranges(), vec![(6, 6)]);
}

#[test]
fn new_file_diff_resolves_old_path_to_dev_null() {
    let d = "--- /dev/null
+++ b/new.py
@@ -0,0 +1,2 @@
+hello
+world
";
    let files = parse(d).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].new_path, "new.py");
    // old_path strips the `a/`/`b/` git prefix; `/dev/null` has neither.
    assert_eq!(files[0].old_path, "/dev/null");
    // The new-file hunk has no removed lines.
    assert!(files[0].hunks[0].removed_ranges().is_empty());
}

#[test]
fn no_newline_at_eof_sentinel_is_ignored() {
    // git-style sentinel after a removed/added line.
    let d = "--- a/foo.py
+++ b/foo.py
@@ -1,1 +1,1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
    let files = parse(d).unwrap();
    let h = &files[0].hunks[0];
    assert_eq!(h.removed_ranges(), vec![(1, 1)]);
}

#[test]
fn removed_ranges_merge_adjacent_and_split_across_context() {
    // Three removed lines (42–44), one context, then two more removed (46–47).
    let h = Hunk {
        old_start: 42,
        old_count: 6,
        new_start: 42,
        new_count: 1,
        lines: vec![
            HunkLine::Removed("a".into()),
            HunkLine::Removed("b".into()),
            HunkLine::Removed("c".into()),
            HunkLine::Context("d".into()),
            HunkLine::Removed("e".into()),
            HunkLine::Removed("f".into()),
        ],
    };
    assert_eq!(h.removed_ranges(), vec![(42, 44), (46, 47)]);
}

#[test]
fn added_lines_do_not_advance_old_line_counter() {
    // Two additions between two removals must NOT push the second
    // removal's old-file line number forward. The removals reference
    // adjacent old-file lines (10 and 11), so they collapse into a
    // single range.
    let h = Hunk {
        old_start: 10,
        old_count: 2,
        new_start: 10,
        new_count: 4,
        lines: vec![
            HunkLine::Removed("a".into()),
            HunkLine::Added("x".into()),
            HunkLine::Added("y".into()),
            HunkLine::Removed("b".into()),
        ],
    };
    assert_eq!(h.removed_ranges(), vec![(10, 11)]);
}

#[test]
fn context_between_removed_splits_runs_but_added_alone_does_not() {
    // Removed(10), Added, Context(11), Removed(12) — the context line
    // advances the old counter past 11, so the second removal lands at
    // 12 and the two ranges are distinct.
    let h = Hunk {
        old_start: 10,
        old_count: 3,
        new_start: 10,
        new_count: 3,
        lines: vec![
            HunkLine::Removed("a".into()),
            HunkLine::Added("x".into()),
            HunkLine::Context("kept".into()),
            HunkLine::Removed("b".into()),
        ],
    };
    assert_eq!(h.removed_ranges(), vec![(10, 10), (12, 12)]);
}

#[test]
fn rejects_malformed_hunk_header() {
    let d = "--- a/foo.py
+++ b/foo.py
@@ broken @@
-x
";
    assert!(parse(d).is_err());
}

/// Property check: every removed range is sorted, non-overlapping, and
/// every line in each range corresponds to a removed line in the hunk.
#[test]
fn removed_ranges_is_well_formed_for_many_random_hunks() {
    // Deterministic pseudo-random over a small space.
    for seed in 0u32..200 {
        let lines = gen_lines(seed);
        let h = Hunk {
            old_start: 1,
            old_count: lines.iter().filter(|l| matches!(l, HunkLine::Removed(_) | HunkLine::Context(_))).count(),
            new_start: 1,
            new_count: lines.iter().filter(|l| matches!(l, HunkLine::Added(_) | HunkLine::Context(_))).count(),
            lines,
        };
        let ranges = h.removed_ranges();
        // sorted + non-overlapping
        for w in ranges.windows(2) {
            assert!(w[0].1 < w[1].0, "ranges overlap: {ranges:?}");
        }
        for (s, e) in &ranges {
            assert!(s <= e, "inverted range");
        }
        // every reported line is a `-` line in `h`.
        let mut expected_remove_lines = Vec::new();
        let mut line = h.old_start;
        for hl in &h.lines {
            match hl {
                HunkLine::Removed(_) => { expected_remove_lines.push(line); line += 1; }
                HunkLine::Context(_) => { line += 1; }
                HunkLine::Added(_) => {}
            }
        }
        let mut flat = Vec::new();
        for (s, e) in &ranges { for n in *s..=*e { flat.push(n); } }
        assert_eq!(flat, expected_remove_lines, "seed={seed}");
    }
}

fn gen_lines(seed: u32) -> Vec<HunkLine> {
    let mut out = Vec::new();
    let mut s = seed;
    for _ in 0..12 {
        s = s.wrapping_mul(1103515245).wrapping_add(12345);
        let kind = (s >> 16) % 3;
        match kind {
            0 => out.push(HunkLine::Context(format!("ctx{s}"))),
            1 => out.push(HunkLine::Removed(format!("del{s}"))),
            _ => out.push(HunkLine::Added(format!("add{s}"))),
        }
    }
    out
}

fn _spotcheck_filediff_construction(d: FileDiff) {
    // Ensures FileDiff is publicly constructible from tests; the field
    // names are the contract.
    let _ = d.old_path;
    let _ = d.new_path;
    let _ = d.hunks;
}
