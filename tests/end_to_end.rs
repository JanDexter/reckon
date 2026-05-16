//! End-to-end test: build a minimal git repo + postmortem in a
//! tempdir, run the memoir engine, then run the tripwire checker
//! against a hand-crafted diff that should reproduce the IR-117 demo
//! moment.
//!
//! This is the most useful test in the suite: it exercises every
//! crate-internal module simultaneously (git, indexer, postmortem,
//! memoir, tripwire, writer). No network: postmortem search is not
//! invoked; the memoir engine resolves incidents through `by_id` and
//! `for_module`, both of which use the offline index.

use std::path::Path;

use git2::{IndexAddOption, Repository, Signature};
use tempfile::TempDir;

use reckon::Reckon;

const RETRY_BEFORE: &str = "\
import time

from . import _gateway

MAX_ATTEMPTS = 3


def _full_jitter(attempt):
    return 0.0


def _build_idempotency_key(order_id, attempt):
    return f\"{order_id}-{attempt}\"


def _persist_key(order_id, attempt, key):
    return None


def _attempt(order_id, attempt):
    backoff = _full_jitter(attempt)
    time.sleep(backoff)
    key = _build_idempotency_key(order_id, attempt)
    _persist_key(order_id, attempt, key)
    return _gateway.charge(order_id, key)
";

const TEST_FILE: &str = "\
def test_idempotency_key_is_deterministic_per_attempt():
    pass
";

const POSTMORTEM: &str = "\
---
id: IR-117
date: 2024-09-04
title: Duplicate charges during partition
services: [payments]
code_refs:
  - { file: \"payments/retry.py\", lines: [22, 23] }
linked_tests:
  - \"tests/test_retry.py::test_idempotency_key_is_deterministic_per_attempt\"
---

# IR-117 — Duplicate charges

A refactor replaced the persisted idempotency key with a request-scoped
UUID generated inside the retry loop. Same charge retried with two keys,
gateway accepted both, customers double-charged.
";

const ISSUES_JSON: &str = "[\
{\"id\": \"IR-117\", \"title\": \"Duplicate charges\", \"state\": \"closed\", \
\"body\": \"see postmortem\", \"labels\": [\"incident\"]}\
]";

const PRS_JSON: &str = "[]";

const DEMO_DIFF: &str = "\
--- a/payments/retry.py
+++ b/payments/retry.py
@@ -20,9 +20,7 @@ def _build_idempotency_key(order_id, attempt):
 def _attempt(order_id, attempt):
     backoff = _full_jitter(attempt)
     time.sleep(backoff)
-    key = _build_idempotency_key(order_id, attempt)
-    _persist_key(order_id, attempt, key)
+    key = \"random\"
     return _gateway.charge(order_id, key)
";

#[test]
fn end_to_end_memoir_then_tripwire() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let repo = Repository::init(root).unwrap();

    // Seed the working tree.
    write(root, "payments/__init__.py", "");
    write(root, "payments/_gateway.py", "def charge(o, k):\n    return f\"{o}:{k}\"\n");
    write(root, "payments/retry.py", RETRY_BEFORE);
    write(root, "tests/test_retry.py", TEST_FILE);
    write(root, "postmortems/IR-117.md", POSTMORTEM);
    write(root, ".fixtures/github/prs.json", PRS_JSON);
    write(root, ".fixtures/github/issues.json", ISSUES_JSON);

    let sig = Signature::now("Test", "test@example.com").unwrap();
    commit_all(&repo, &sig, "feat: initial payments retry with idempotency (IR-117)");

    let reckon = Reckon::open(root).expect("open reckon");

    // Memoir generation discovers the IR-117 incident from the commit
    // message and links it to the postmortem's code_refs.
    let md = reckon.memoir.update_for_path(Path::new("payments/retry.py")).unwrap();
    assert!(md.contains("module: payments"), "memoir missing module key:\n{md}");
    assert!(md.contains("incident: IR-117"), "memoir missing IR-117 tripwire:\n{md}");
    assert!(md.contains("payments/retry.py"), "memoir missing region file:\n{md}");

    // Round-trip parse: written memoir is valid YAML + body.
    let parsed = reckon
        .memoir
        .read(Path::new("payments"))
        .unwrap()
        .expect("memoir present on disk");
    let tripwires = parsed.frontmatter.tripwires;
    assert!(!tripwires.is_empty(), "no tripwires extracted");
    let tw = tripwires
        .iter()
        .find(|t| t.incident == "IR-117")
        .expect("IR-117 tripwire");
    assert_eq!(tw.region.file, "payments/retry.py");
    assert_eq!(tw.region.lines, [22, 23]);
    assert_eq!(
        tw.guarding_test,
        "tests/test_retry.py::test_idempotency_key_is_deterministic_per_attempt"
    );

    // Tripwire check: a diff that removes the protected lines must
    // produce one warning whose incident is IR-117.
    let result = reckon
        .tripwire
        .check(DEMO_DIFF, &reckon.memoir, &reckon.postmortems, "test-trace")
        .unwrap();
    assert_eq!(result.warnings.len(), 1, "expected one warning, got {result:?}");
    let w = &result.warnings[0];
    assert_eq!(w.incident, "IR-117");
    assert_eq!(w.tripwire_id, tw.id);
    assert_eq!(w.severity, "high");
    assert_eq!(w.suggested_action, "revert_hunk");
    assert_eq!(w.guarding_test, tw.guarding_test);
    assert!(w.memoir_quote.contains("Duplicate charges"));
    assert_eq!(w.file, "payments/retry.py");
    let [s, e] = w.removed_range;
    // The patched hunk removes 2 adjacent lines starting at line 23 of
    // the original file (`_build_idempotency_key` + `_persist_key`).
    assert!(s <= 23 && e >= 23, "removed range {s}..{e} should cover line 23");
}

#[test]
fn check_against_unrelated_diff_produces_no_warnings() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let repo = Repository::init(root).unwrap();
    write(root, "payments/__init__.py", "");
    write(root, "payments/_gateway.py", "def charge(o, k):\n    return o\n");
    write(root, "payments/retry.py", RETRY_BEFORE);
    write(root, "tests/test_retry.py", TEST_FILE);
    write(root, "postmortems/IR-117.md", POSTMORTEM);
    write(root, ".fixtures/github/prs.json", PRS_JSON);
    write(root, ".fixtures/github/issues.json", ISSUES_JSON);

    let sig = Signature::now("Test", "test@example.com").unwrap();
    commit_all(&repo, &sig, "feat: initial payments retry (IR-117)");

    let reckon = Reckon::open(root).unwrap();
    let _ = reckon.memoir.update_for_path(Path::new("payments/retry.py")).unwrap();

    let unrelated_diff = "\
--- a/payments/_gateway.py
+++ b/payments/_gateway.py
@@ -1,2 +1,2 @@
 def charge(o, k):
-    return o
+    return f\"{o}:{k}\"
";
    let result = reckon
        .tripwire
        .check(unrelated_diff, &reckon.memoir, &reckon.postmortems, "test-trace")
        .unwrap();
    assert!(
        result.warnings.is_empty(),
        "expected no warnings on unrelated diff, got {:?}",
        result.warnings,
    );
}

#[test]
fn memoir_round_trip_through_disk_preserves_tripwires() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let repo = Repository::init(root).unwrap();
    write(root, "payments/__init__.py", "");
    write(root, "payments/_gateway.py", "def charge(o, k):\n    return o\n");
    write(root, "payments/retry.py", RETRY_BEFORE);
    write(root, "tests/test_retry.py", TEST_FILE);
    write(root, "postmortems/IR-117.md", POSTMORTEM);
    write(root, ".fixtures/github/prs.json", PRS_JSON);
    write(root, ".fixtures/github/issues.json", ISSUES_JSON);

    let sig = Signature::now("Test", "test@example.com").unwrap();
    commit_all(&repo, &sig, "feat: initial payments retry (IR-117)");

    let reckon = Reckon::open(root).unwrap();
    let first = reckon.memoir.update_for_path(Path::new("payments/retry.py")).unwrap();
    let parsed = reckon.memoir.read(Path::new("payments")).unwrap().unwrap();

    // The body of `update_for_path` is the canonical render; the parsed
    // copy holds the same tripwires.
    assert!(first.contains("tripwires:"));
    assert_eq!(parsed.frontmatter.module, "payments");
    assert!(!parsed.frontmatter.tripwires.is_empty());
}

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, contents).unwrap();
}

fn commit_all(repo: &Repository, sig: &Signature<'_>, msg: &str) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["."].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit<'_>> = parent.as_ref().into_iter().collect();
    repo.commit(Some("HEAD"), sig, sig, msg, &tree, &parents)
        .unwrap()
}
