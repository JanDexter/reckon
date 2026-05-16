//! Unified-diff parser focused on what `check_diff` needs: for each
//! affected file, the line ranges that have been removed in the OLD file.
//!
//! Format target: standard unified diff (`@@ -a,b +c,d @@`).

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<HunkLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLine {
    Context(String),
    Removed(String),
    Added(String),
}

impl Hunk {
    /// Inclusive line ranges in the OLD file that were removed.
    /// Adjacent removed lines collapse into one range.
    pub fn removed_ranges(&self) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut line = self.old_start;
        let mut run: Option<(usize, usize)> = None;
        for hl in &self.lines {
            match hl {
                HunkLine::Removed(_) => {
                    run = Some(match run {
                        Some((s, _)) => (s, line),
                        None => (line, line),
                    });
                    line += 1;
                }
                HunkLine::Context(_) => {
                    if let Some(r) = run.take() {
                        out.push(r);
                    }
                    line += 1;
                }
                HunkLine::Added(_) => {}
            }
        }
        if let Some(r) = run.take() {
            out.push(r);
        }
        out
    }
}

pub fn parse(input: &str) -> Result<Vec<FileDiff>> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur: Option<FileDiff> = None;
    let mut cur_hunk: Option<Hunk> = None;
    let mut old_path: Option<String> = None;

    for raw in input.lines() {
        if let Some(rest) = raw.strip_prefix("--- ") {
            // commit a previous file
            if let Some(h) = cur_hunk.take() {
                if let Some(f) = cur.as_mut() {
                    f.hunks.push(h);
                }
            }
            if let Some(f) = cur.take() {
                files.push(f);
            }
            old_path = Some(strip_path_prefix(rest));
        } else if let Some(rest) = raw.strip_prefix("+++ ") {
            let new_path = strip_path_prefix(rest);
            let op = old_path
                .take()
                .ok_or_else(|| Error::DiffParse("'+++' without preceding '---'".into()))?;
            cur = Some(FileDiff {
                old_path: op,
                new_path,
                hunks: Vec::new(),
            });
        } else if let Some(rest) = raw.strip_prefix("@@ ") {
            if let Some(h) = cur_hunk.take() {
                if let Some(f) = cur.as_mut() {
                    f.hunks.push(h);
                }
            }
            cur_hunk = Some(parse_hunk_header(rest)?);
        } else if let Some(h) = cur_hunk.as_mut() {
            if let Some(line) = raw.strip_prefix('-') {
                if raw.starts_with("---") {
                    // file marker, ignored (handled above)
                } else {
                    h.lines.push(HunkLine::Removed(line.to_string()));
                }
            } else if let Some(line) = raw.strip_prefix('+') {
                if raw.starts_with("+++") {
                    // ignored
                } else {
                    h.lines.push(HunkLine::Added(line.to_string()));
                }
            } else if let Some(line) = raw.strip_prefix(' ') {
                h.lines.push(HunkLine::Context(line.to_string()));
            } else if raw.is_empty() {
                h.lines.push(HunkLine::Context(String::new()));
            }
            // git extended headers ("\\ No newline at end of file") just ignored
        }
    }
    if let Some(h) = cur_hunk.take() {
        if let Some(f) = cur.as_mut() {
            f.hunks.push(h);
        }
    }
    if let Some(f) = cur.take() {
        files.push(f);
    }
    Ok(files)
}

fn strip_path_prefix(s: &str) -> String {
    let s = s.trim();
    // Strip "a/" or "b/" git prefix if present.
    if let Some(rest) = s.strip_prefix("a/").or_else(|| s.strip_prefix("b/")) {
        rest.to_string()
    } else {
        s.to_string()
    }
}

fn parse_hunk_header(rest: &str) -> Result<Hunk> {
    // Format: `-a,b +c,d @@ optional context`
    let end = rest
        .find(" @@")
        .ok_or_else(|| Error::DiffParse(format!("malformed hunk header: {rest}")))?;
    let header = &rest[..end];
    let mut old = None;
    let mut new = None;
    for tok in header.split_whitespace() {
        if let Some(t) = tok.strip_prefix('-') {
            old = Some(parse_range(t)?);
        } else if let Some(t) = tok.strip_prefix('+') {
            new = Some(parse_range(t)?);
        }
    }
    let (os, oc) = old.ok_or_else(|| Error::DiffParse("missing - range in hunk".into()))?;
    let (ns, nc) = new.ok_or_else(|| Error::DiffParse("missing + range in hunk".into()))?;
    Ok(Hunk {
        old_start: os,
        old_count: oc,
        new_start: ns,
        new_count: nc,
        lines: Vec::new(),
    })
}

fn parse_range(s: &str) -> Result<(usize, usize)> {
    let mut it = s.split(',');
    let start: usize = it
        .next()
        .ok_or_else(|| Error::DiffParse(format!("empty range {s}")))?
        .parse()
        .map_err(|_| Error::DiffParse(format!("bad range {s}")))?;
    let count: usize = match it.next() {
        Some(c) => c
            .parse()
            .map_err(|_| Error::DiffParse(format!("bad count {s}")))?,
        None => 1,
    };
    Ok((start, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "--- a/payments/retry.py
+++ b/payments/retry.py
@@ -40,9 +40,7 @@ def _attempt(order_id, attempt):
 def _attempt(order_id, attempt):
     backoff = _full_jitter(attempt)
     time.sleep(backoff)
-    key = _build_idempotency_key(order_id, attempt)
-    _persist_key(order_id, attempt, key)
+    key = uuid.uuid4().hex
     return _gateway.charge(order_id, key)
";

    #[test]
    fn parses_sample() {
        let files = parse(SAMPLE).unwrap();
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.old_path, "payments/retry.py");
        assert_eq!(f.new_path, "payments/retry.py");
        assert_eq!(f.hunks.len(), 1);
        let ranges = f.hunks[0].removed_ranges();
        // Removed lines are at old lines 43-44 inclusive.
        assert_eq!(ranges, vec![(43, 44)]);
    }
}
