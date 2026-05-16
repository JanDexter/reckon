//! Tripwire helper tests: region overlap math and file-path normalization.

use reckon::tripwire::{ranges_overlap, same_file};

#[test]
fn ranges_overlap_basic() {
    assert!(ranges_overlap(1, 5, 3, 4));
    assert!(ranges_overlap(1, 5, 5, 8));   // touching is overlap
    assert!(ranges_overlap(5, 8, 1, 5));   // touching is overlap, reversed
    assert!(ranges_overlap(1, 1, 1, 1));   // single-line overlap
    assert!(!ranges_overlap(1, 5, 6, 8));
    assert!(!ranges_overlap(6, 8, 1, 5));
}

/// Brute-force every pair in a small range. The overlap relation must
/// match the textbook definition: `a0 <= b1 && b0 <= a1`.
#[test]
fn ranges_overlap_exhaustive_small_space() {
    const N: usize = 12;
    for a0 in 1..=N {
        for a1 in a0..=N {
            for b0 in 1..=N {
                for b1 in b0..=N {
                    let got = ranges_overlap(a0, a1, b0, b1);
                    let want = a0 <= b1 && b0 <= a1;
                    assert_eq!(
                        got, want,
                        "({a0},{a1}) vs ({b0},{b1}): got {got}, want {want}"
                    );
                }
            }
        }
    }
}

#[test]
fn ranges_overlap_is_symmetric() {
    for a0 in 1..=8usize {
        for a1 in a0..=8 {
            for b0 in 1..=8 {
                for b1 in b0..=8 {
                    assert_eq!(
                        ranges_overlap(a0, a1, b0, b1),
                        ranges_overlap(b0, b1, a0, a1),
                        "symmetry broken: ({a0},{a1}) vs ({b0},{b1})"
                    );
                }
            }
        }
    }
}

#[test]
fn same_file_handles_identical_paths() {
    assert!(same_file("payments/retry.py", "payments/retry.py"));
}

#[test]
fn same_file_normalizes_windows_separators() {
    assert!(same_file("payments\\retry.py", "payments/retry.py"));
    assert!(same_file("payments/retry.py", "payments\\retry.py"));
}

#[test]
fn same_file_matches_on_suffix() {
    // diff old_path is often relative (`payments/retry.py`) while a
    // tripwire region.file may be repo-rooted (also `payments/retry.py`).
    // Matching on suffix is a deliberate POC convenience.
    assert!(same_file("payments/retry.py", "src/payments/retry.py"));
    assert!(same_file("src/payments/retry.py", "payments/retry.py"));
}

#[test]
fn same_file_rejects_unrelated_paths() {
    assert!(!same_file("payments/retry.py", "payments/charge.py"));
    assert!(!same_file("foo.py", "bar.py"));
}
