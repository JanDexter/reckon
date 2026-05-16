//! Tripwire Checker.
//!
//! Loads tripwires from a memoir's frontmatter, parses a unified diff, and
//! emits one warning per (hunk × tripwire) intersection. The warning shape
//! matches §7 of the architecture.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::config::Config;
use crate::diff;
use crate::memoir::{MemoirEngine, TripwireSpec};
use crate::postmortem::PostmortemIndex;
use crate::Result;

pub struct TripwireChecker {
    pub config: Arc<Config>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Warning {
    pub tripwire_id: String,
    pub severity: String,
    pub incident: String,
    pub memoir_quote: String,
    pub guarding_test: String,
    pub test_assertion: String,
    pub suggested_action: String,
    pub trace_id: String,
    pub file: String,
    pub removed_range: [usize; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub warnings: Vec<Warning>,
}

impl TripwireChecker {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Check a unified diff against every applicable tripwire.
    ///
    /// `engine` and `postmortems` are passed in so the checker can load
    /// memoirs and quote-relevant scars without owning either subsystem.
    pub fn check(
        &self,
        diff_text: &str,
        engine: &MemoirEngine,
        postmortems: &PostmortemIndex,
        trace_id: &str,
    ) -> Result<CheckResult> {
        let files = diff::parse(diff_text)?;
        let mut warnings = Vec::new();

        for file in &files {
            let path = Path::new(&file.old_path);
            // Find the module containing this file and load its memoir.
            let module_dir = match engine.indexer.module_for_path(path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let parsed = match engine.read(&module_dir)? {
                Some(p) => p,
                None => continue,
            };

            for hunk in &file.hunks {
                for (rs, re) in hunk.removed_ranges() {
                    for tw in &parsed.frontmatter.tripwires {
                        if !same_file(&tw.region.file, &file.old_path) {
                            continue;
                        }
                        let (ts, te) = (tw.region.lines[0], tw.region.lines[1]);
                        if !ranges_overlap(rs, re, ts, te) {
                            continue;
                        }
                        let scar = postmortems.by_id(&tw.incident);
                        let memoir_quote = quote_for_tripwire(tw, scar.as_deref());
                        let test_assertion = scar
                            .and_then(|pm| first_sentence(&pm.body))
                            .unwrap_or_else(|| "guarding test asserts the prior fix still holds".into());

                        warnings.push(Warning {
                            tripwire_id: tw.id.clone(),
                            severity: "high".into(),
                            incident: tw.incident.clone(),
                            memoir_quote,
                            guarding_test: tw.guarding_test.clone(),
                            test_assertion,
                            suggested_action: "revert_hunk".into(),
                            trace_id: trace_id.to_string(),
                            file: file.old_path.clone(),
                            removed_range: [rs, re],
                        });
                    }
                }
            }
        }

        Ok(CheckResult { warnings })
    }
}

#[doc(hidden)]
pub fn same_file(a: &str, b: &str) -> bool {
    let na = a.replace('\\', "/");
    let nb = b.replace('\\', "/");
    na == nb || na.ends_with(&nb) || nb.ends_with(&na)
}

#[doc(hidden)]
pub fn ranges_overlap(a0: usize, a1: usize, b0: usize, b1: usize) -> bool {
    a0 <= b1 && b0 <= a1
}

fn quote_for_tripwire(tw: &TripwireSpec, scar: Option<&crate::postmortem::Postmortem>) -> String {
    if let Some(pm) = scar {
        format!(
            "{title} — {first}",
            title = pm.fm.title,
            first = first_paragraph_trim(&pm.body, 240)
        )
    } else {
        tw.summary.clone()
    }
}

fn first_paragraph_trim(s: &str, limit: usize) -> String {
    let para = first_prose_paragraph(s);
    if para.chars().count() <= limit {
        para
    } else {
        let trimmed: String = para.chars().take(limit).collect();
        format!("{trimmed}…")
    }
}

fn first_sentence(s: &str) -> Option<String> {
    let para = first_prose_paragraph(s);
    if para.is_empty() {
        return None;
    }
    let end = para.find(". ").map(|i| i + 1).unwrap_or(para.len());
    Some(para[..end].trim().to_string())
}

/// First paragraph that isn't a Markdown heading (`#`, `##`, …).
fn first_prose_paragraph(s: &str) -> String {
    for para in s.split("\n\n") {
        let trimmed = para.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed.replace('\n', " ");
    }
    String::new()
}
