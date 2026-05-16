//! Demo-repo seeder.
//!
//! Builds the synthetic git repository the architecture's §10 build order
//! expects everything else to be grounded in. Idempotent: pass `--force` to
//! wipe and recreate.
//!
//! Output layout (under `--out`):
//!
//!   demo-repo/
//!     payments/
//!       __init__.py
//!       _gateway.py
//!       retry.py
//!     tests/payments/test_retry.py
//!     postmortems/IR-117.md
//!     postmortems/IR-091.md
//!     .fixtures/github/prs.json
//!     .fixtures/github/issues.json
//!     README.md
//!
//! Final HEAD: retry.py contains the persisted idempotency key. The demo
//! diff (removing the key) intersects the tripwire generated for IR-117.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use git2::{Repository, Signature};

#[derive(Parser, Debug)]
#[command(name = "reckon-seed", about = "Seed the Reckon demo repository.")]
struct Args {
    /// Output directory. Will be created if missing.
    #[arg(long, default_value = "demo-repo")]
    out: PathBuf,
    /// Wipe `out` before seeding.
    #[arg(long, default_value_t = false)]
    force: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.out.exists() {
        if args.force {
            fs::remove_dir_all(&args.out)
                .with_context(|| format!("wiping {}", args.out.display()))?;
        } else {
            return Err(anyhow!(
                "{} already exists. Pass --force to overwrite.",
                args.out.display()
            ));
        }
    }
    fs::create_dir_all(&args.out)?;
    let repo = Repository::init(&args.out).context("git init")?;
    let signature = Signature::now("Bob Reckon", "bob-reckon@example.com")?;

    // C0 — initial commit: basic retry with no idempotency, no cap.
    write_files(&args.out, &INITIAL_FILES)?;
    let c0 = commit(&repo, &signature, "feat(payments): initial retry shell", None)?;

    // C1 — IR-091 retrospective: cap retries, full jitter, add storm test.
    write_files(&args.out, &C1_FILES)?;
    let c1 = commit(
        &repo,
        &signature,
        "fix(retry): cap retries at 3 and switch to full jitter (IR-091)\n\nAfter the Q3 Stripe degradation we retried 4× with equal jitter; \
        outbound load multiplied ~30× and we got rate-limited for 14m. \
        Drop the cap to 3 and use full jitter so backoff windows decorrelate.",
        Some(c0),
    )?;

    // C2 — IR-117 fix: persist idempotency key. Adds the guarding test in
    // the SAME change — this is what makes the tripwire generator pick it
    // up.
    write_files(&args.out, &C2_FILES)?;
    let c2 = commit(
        &repo,
        &signature,
        "fix(retry): persist idempotency key before network call (IR-117)\n\nA refactor had replaced the persisted key with a request-scoped UUID. \
        Under a 90s partition the same charge retried with two different keys \
        and Stripe accepted both as distinct PaymentIntents. 312 customers \
        were double-charged. Re-persist the key keyed by (order_id, attempt) \
        before any network call. Adds test_idempotency_key_is_deterministic_per_attempt.",
        Some(c1),
    )?;

    // C3 — small refinement.
    write_files(&args.out, &C3_FILES)?;
    let _c3 = commit(
        &repo,
        &signature,
        "chore(retry): skip retry on permanent_decline classifier",
        Some(c2),
    )?;

    // Forge + postmortem fixtures (written after the final commit so they
    // do not appear as a commit themselves — the indexer reads them out of
    // band).
    let merge_commit_c2 = head_sha(&repo)?;
    let _ = merge_commit_c2; // only used inside the fixtures below
    write_files(&args.out, &fixtures(head_short(&repo)?))?;

    println!(
        "Seeded {} with {} commits. HEAD: {}",
        args.out.display(),
        4,
        head_short(&repo)?
    );
    println!("\nNext steps:");
    println!("  cd {}", args.out.display());
    println!("  RECKON_REPO=$PWD cargo run --bin reckon-mcp");
    Ok(())
}

fn write_files<S: AsRef<str>>(root: &Path, files: &[(&str, S)]) -> Result<()> {
    for (rel, contents) in files {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&p, contents.as_ref().as_bytes())
            .with_context(|| format!("writing {}", p.display()))?;
    }
    Ok(())
}

fn commit(
    repo: &Repository,
    sig: &Signature,
    message: &str,
    parent_oid: Option<git2::Oid>,
) -> Result<git2::Oid> {
    let mut index = repo.index()?;
    index.add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let parents: Vec<git2::Commit<'_>> = match parent_oid {
        Some(oid) => vec![repo.find_commit(oid)?],
        None => Vec::new(),
    };
    let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();
    let oid = repo.commit(Some("HEAD"), sig, sig, message, &tree, &parent_refs)?;
    Ok(oid)
}

fn head_sha(repo: &Repository) -> Result<String> {
    let h = repo.head()?.peel_to_commit()?;
    Ok(h.id().to_string())
}

fn head_short(repo: &Repository) -> Result<String> {
    let s = head_sha(repo)?;
    Ok(s.chars().take(7).collect())
}

// ───────────────────────── file payloads ─────────────────────────

const INITIAL_INIT: &str = "from .retry import retry_payment  # noqa: F401\n";

const INITIAL_GATEWAY: &str = r#""""Test stub for the payment gateway adapter."""


def charge(order_id: str, idempotency_key: str) -> str:
    return f"charge:{order_id}:{idempotency_key}"
"#;

const INITIAL_RETRY: &str = r#""""payments.retry — initial naive retry."""

import time

from . import _gateway


def retry_payment(order_id: str) -> str:
    for attempt in range(5):
        try:
            return _gateway.charge(order_id, "")
        except Exception:
            time.sleep(1.0)
    raise RuntimeError("max retries exceeded")
"#;

const INITIAL_TEST: &str = r#"from payments.retry import retry_payment


def test_retry_payment_returns_a_value():
    assert retry_payment("ord-1") is not None
"#;

const INITIAL_FILES: &[(&str, &str)] = &[
    ("payments/__init__.py", INITIAL_INIT),
    ("payments/_gateway.py", INITIAL_GATEWAY),
    ("payments/retry.py", INITIAL_RETRY),
    ("tests/payments/test_retry.py", INITIAL_TEST),
    ("README.md", "# Demo Repo\n\nA synthetic payments service used by the Reckon POC.\n"),
];

// After IR-091: cap + jitter.
const C1_RETRY: &str = r#""""payments.retry — capped retry with full jitter."""

import random
import time

from . import _gateway

MAX_ATTEMPTS = 3


def _full_jitter(attempt: int) -> float:
    cap = min(30.0, 2.0 ** attempt)
    return random.uniform(0.0, cap)


def retry_payment(order_id: str) -> str:
    for attempt in range(MAX_ATTEMPTS):
        try:
            time.sleep(_full_jitter(attempt))
            return _gateway.charge(order_id, "")
        except Exception:
            continue
    raise RuntimeError("max retries exceeded")
"#;

const C1_TEST: &str = r#"from payments.retry import MAX_ATTEMPTS, retry_payment


def test_retry_payment_returns_a_value():
    assert retry_payment("ord-1") is not None


def test_retry_cap_holds_under_partition():
    # The retry shell must not exceed MAX_ATTEMPTS outbound charges even
    # under a simulated network partition (IR-091).
    assert MAX_ATTEMPTS == 3
"#;

const C1_FILES: &[(&str, &str)] = &[
    ("payments/retry.py", C1_RETRY),
    ("tests/payments/test_retry.py", C1_TEST),
];

// After IR-117: persisted idempotency key.
const C2_RETRY: &str = r#""""payments.retry — capped retry with persisted idempotency key.

The idempotency key shape is load-bearing — it is the fix from IR-117.
Removing the `_build_idempotency_key` / `_persist_key` pair re-opens the
double-charge failure mode. See `payments/MEMOIR.md`.
"""

import random
import time

from . import _gateway

MAX_ATTEMPTS = 3


def _full_jitter(attempt: int) -> float:
    cap = min(30.0, 2.0 ** attempt)
    return random.uniform(0.0, cap)


def _build_idempotency_key(order_id: str, attempt: int) -> str:
    """Derive the per-attempt idempotency key. Must be deterministic in
    `(order_id, attempt)` so a retry across a process boundary still
    produces the same key."""
    return f"{order_id}-{attempt}"


def _persist_key(order_id: str, attempt: int, key: str) -> None:
    """Persist the key to durable storage BEFORE the network call so a
    crash mid-charge still produces the same key on the next worker."""
    # POC: write to .reckon-demo-keys/ (real impl: durable store).
    return None


def _attempt(order_id: str, attempt: int) -> str:
    backoff = _full_jitter(attempt)
    time.sleep(backoff)
    key = _build_idempotency_key(order_id, attempt)
    _persist_key(order_id, attempt, key)
    return _gateway.charge(order_id, key)


def retry_payment(order_id: str) -> str:
    for attempt in range(MAX_ATTEMPTS):
        try:
            return _attempt(order_id, attempt)
        except Exception:
            continue
    raise RuntimeError("max retries exceeded")
"#;

const C2_TEST: &str = r#"from payments.retry import (
    MAX_ATTEMPTS,
    _build_idempotency_key,
    retry_payment,
)


def test_retry_payment_returns_a_value():
    assert retry_payment("ord-1") is not None


def test_retry_cap_holds_under_partition():
    assert MAX_ATTEMPTS == 3


def test_idempotency_key_is_deterministic_per_attempt():
    """IR-117 regression guard: same (order_id, attempt) must always yield
    the same key, across processes, attempts, and partitions."""
    a = _build_idempotency_key("ord-42", 0)
    b = _build_idempotency_key("ord-42", 0)
    assert a == b
    c = _build_idempotency_key("ord-42", 1)
    assert a != c
"#;

const C2_FILES: &[(&str, &str)] = &[
    ("payments/retry.py", C2_RETRY),
    ("tests/payments/test_retry.py", C2_TEST),
];

// C3 — permanent decline classifier.
const C3_RETRY: &str = r#""""payments.retry — capped retry with persisted idempotency key.

The idempotency key shape is load-bearing — it is the fix from IR-117.
Removing the `_build_idempotency_key` / `_persist_key` pair re-opens the
double-charge failure mode. See `payments/MEMOIR.md`.
"""

import random
import time

from . import _gateway

MAX_ATTEMPTS = 3
PERMANENT_ERRORS = {"card_declined", "fraudulent"}


def _full_jitter(attempt: int) -> float:
    cap = min(30.0, 2.0 ** attempt)
    return random.uniform(0.0, cap)


def _classify_error(err: object) -> str:
    name = getattr(err, "code", None) or getattr(err, "__class__", type(err)).__name__
    return "permanent" if name in PERMANENT_ERRORS else "transient"


def _build_idempotency_key(order_id: str, attempt: int) -> str:
    """Derive the per-attempt idempotency key. Must be deterministic in
    `(order_id, attempt)` so a retry across a process boundary still
    produces the same key."""
    return f"{order_id}-{attempt}"


def _persist_key(order_id: str, attempt: int, key: str) -> None:
    """Persist the key to durable storage BEFORE the network call so a
    crash mid-charge still produces the same key on the next worker."""
    return None


def _attempt(order_id: str, attempt: int) -> str:
    backoff = _full_jitter(attempt)
    time.sleep(backoff)
    key = _build_idempotency_key(order_id, attempt)
    _persist_key(order_id, attempt, key)
    return _gateway.charge(order_id, key)


def retry_payment(order_id: str) -> str:
    for attempt in range(MAX_ATTEMPTS):
        try:
            return _attempt(order_id, attempt)
        except Exception as exc:
            if _classify_error(exc) == "permanent":
                raise
            continue
    raise RuntimeError("max retries exceeded")
"#;

const C3_FILES: &[(&str, &str)] = &[("payments/retry.py", C3_RETRY)];

// ───────────────────────── postmortems + fixtures ─────────────────────────

fn fixtures(head_short: String) -> Vec<(&'static str, String)> {
    // Postmortem code_refs point at the persisted-key region in retry.py.
    // In C3_RETRY those lines are 27–28 (idempotency-key build + persist).
    let ir117 = r##"---
id: IR-117
date: 2024-09-04
title: Duplicate charges during a network partition
services:
  - payments
code_refs:
  - { file: "payments/retry.py", lines: [27, 28] }
linked_prs:
  - "#2847"
linked_tests:
  - "tests/payments/test_retry.py::test_idempotency_key_is_deterministic_per_attempt"
---

# IR-117 — Duplicate charges during a network partition

## Summary

A refactor replaced the persisted idempotency key in `payments/retry.py`
with a request-scoped UUID generated inside the retry loop. Under a 90s
partition between us and Stripe, the same charge retried with two
different keys and Stripe accepted both as distinct PaymentIntents.
312 customers were double-charged. ≈$48K refunded manually.

## Fix

Persist the idempotency key, keyed by `(order_id, attempt_index)`,
**before** any network call. The regression is held by
`tests/payments/test_retry.py::test_idempotency_key_is_deterministic_per_attempt`.

## Why this still matters

Anything generated inside the retry loop can diverge across attempts; the
key has to be derived from inputs that survive a retry, and it has to be
written down before the network call so a process crash mid-charge still
produces the same key on the next worker.
"##;

    let ir091 = r##"---
id: IR-091
date: 2024-06-22
title: Retry storm during Stripe degradation
services:
  - payments
code_refs:
  - { file: "payments/retry.py", lines: [11, 11] }
linked_prs:
  - "#2611"
linked_tests:
  - "tests/payments/test_retry.py::test_retry_cap_holds_under_partition"
---

# IR-091 — Retry storm during Stripe degradation

## Summary

The original retry loop used equal jitter and a 4-attempt cap. When
Stripe latency rose to ~8s p99, our outbound load multiplied ~30× and
Stripe rate-limited us for 14 minutes.

## Fix

Reduce cap to 3 attempts. Switch jitter from equal to full so retry
windows decorrelate. The regression is held by
`tests/payments/test_retry.py::test_retry_cap_holds_under_partition`.
"##;

    let prs = format!(
        r##"[
  {{
    "number": 2611,
    "title": "Retry: cap at 3 + full jitter (IR-091)",
    "state": "merged",
    "author": "kw",
    "merged_at": "2024-06-23T18:11:00Z",
    "merge_commit": "{head}",
    "body": "Closes IR-091. Reduces MAX_ATTEMPTS to 3 and replaces equal jitter with full jitter so retry windows decorrelate.",
    "linked_incidents": ["IR-091"],
    "linked_tests": ["tests/payments/test_retry.py::test_retry_cap_holds_under_partition"],
    "files": ["payments/retry.py", "tests/payments/test_retry.py"]
  }},
  {{
    "number": 2847,
    "title": "Retry: persist idempotency key before network call (IR-117)",
    "state": "merged",
    "author": "kw",
    "merged_at": "2024-09-05T09:30:00Z",
    "merge_commit": "{head}",
    "body": "Closes IR-117. Persists the per-attempt idempotency key derived from (order_id, attempt) before any network call.",
    "linked_incidents": ["IR-117"],
    "linked_tests": ["tests/payments/test_retry.py::test_idempotency_key_is_deterministic_per_attempt"],
    "files": ["payments/retry.py", "tests/payments/test_retry.py"]
  }},
  {{
    "number": 2841,
    "title": "Switch retry loop to the `tenacity` library",
    "state": "closed",
    "author": "kw",
    "closed_at": "2024-10-12T14:00:00Z",
    "body": "Tenacity re-raises on permanent errors before our classifier runs, which would re-open the IR-117 failure mode. Closing.",
    "linked_incidents": ["IR-117"],
    "files": ["payments/retry.py"]
  }},
  {{
    "number": 3104,
    "title": "Make MAX_ATTEMPTS configurable per tenant",
    "state": "closed",
    "author": "kw",
    "closed_at": "2025-01-08T11:22:00Z",
    "body": "Two tenants would have raised it to 6+ within a week; ops review surfaced thundering-herd risk identical to IR-091.",
    "linked_incidents": ["IR-091"],
    "files": ["payments/retry.py"]
  }},
  {{
    "number": 3318,
    "title": "Replace persisted key with Redis-backed UUID lease",
    "state": "closed",
    "author": "kw",
    "closed_at": "2025-03-22T16:40:00Z",
    "body": "Adds a runtime dependency to a code path that must succeed when Redis is down. Closed in favor of keeping the key derivation pure.",
    "linked_incidents": ["IR-117"],
    "files": ["payments/retry.py"]
  }}
]
"##,
        head = head_short
    );

    let issues = r##"[
  {
    "id": "IR-117",
    "title": "Duplicate charges during a network partition",
    "state": "closed",
    "body": "See postmortems/IR-117.md.",
    "labels": ["incident", "severity-high"],
    "closed_at": "2024-09-05T09:30:00Z"
  },
  {
    "id": "IR-091",
    "title": "Retry storm during Stripe degradation",
    "state": "closed",
    "body": "See postmortems/IR-091.md.",
    "labels": ["incident", "severity-high"],
    "closed_at": "2024-06-23T18:11:00Z"
  }
]
"##;

    vec![
        ("postmortems/IR-117.md", ir117.to_string()),
        ("postmortems/IR-091.md", ir091.to_string()),
        (".fixtures/github/prs.json", prs),
        (".fixtures/github/issues.json", issues.to_string()),
    ]
}
