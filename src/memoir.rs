//! Memoir Engine.
//!
//! Generates and updates `<module>/MEMOIR.md`. The frontmatter is the
//! machine-readable layer (`tripwires` live here — that is what
//! `check_diff` matches against). The body is the human-readable layer.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::indexer::{ArtifactBundle, ArtifactIndexer, PullRequest};
use crate::postmortem::{split_frontmatter, Postmortem, PostmortemIndex};
use crate::writer::atomic_write;
use crate::{Error, Result};

pub const MEMOIR_FILENAME: &str = "MEMOIR.md";

pub struct MemoirEngine {
    pub config: Arc<Config>,
    pub indexer: Arc<ArtifactIndexer>,
    pub postmortems: Arc<PostmortemIndex>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoirFrontmatter {
    pub module: String,
    pub last_updated: String,
    pub last_commit: String,
    pub evidence_tier: String, // "strong" | "moderate" | "weak"
    #[serde(default)]
    pub tripwires: Vec<TripwireSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripwireSpec {
    pub id: String,
    pub region: RegionSpec,
    pub incident: String,
    pub guarding_test: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionSpec {
    pub file: String,
    pub lines: [usize; 2],
}

/// Convenience struct for reading an existing memoir without re-rendering it.
#[derive(Debug, Clone)]
pub struct ParsedMemoir {
    pub frontmatter: MemoirFrontmatter,
    pub body: String,
}

impl MemoirEngine {
    pub fn new(
        config: Arc<Config>,
        indexer: Arc<ArtifactIndexer>,
        postmortems: Arc<PostmortemIndex>,
    ) -> Self {
        Self { config, indexer, postmortems }
    }

    pub fn memoir_path(&self, module_dir: &Path) -> PathBuf {
        self.config.repo_root.join(module_dir).join(MEMOIR_FILENAME)
    }

    /// Read an existing memoir from disk. Returns `None` if absent.
    pub fn read(&self, module_dir: &Path) -> Result<Option<ParsedMemoir>> {
        let p = self.memoir_path(module_dir);
        if !p.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&p)?;
        let (fm_str, body) = split_frontmatter(&raw)?;
        let fm: MemoirFrontmatter = if fm_str.is_empty() {
            return Err(Error::InvalidMemoir(format!(
                "{} has no frontmatter",
                p.display()
            )));
        } else {
            serde_yaml::from_str(&fm_str)?
        };
        Ok(Some(ParsedMemoir {
            frontmatter: fm,
            body: body.to_string(),
        }))
    }

    /// Regenerate the memoir for the module containing `path`. Writes
    /// atomically and returns the rendered markdown.
    ///
    /// Refuses paths that don't exist anywhere under the repo so an MCP
    /// tool call against a phantom path can't scaffold a ghost module.
    pub fn update_for_path(&self, path: &Path) -> Result<String> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.config.repo_root.join(path)
        };
        let parent = abs.parent().unwrap_or(&self.config.repo_root);
        if !abs.exists() && !parent.exists() {
            return Err(Error::ModuleNotFound(path.to_path_buf()));
        }
        let module_dir = self.indexer.module_for_path(path)?;
        let module_abs = self.config.repo_root.join(&module_dir);
        if !module_abs.exists() {
            return Err(Error::ModuleNotFound(path.to_path_buf()));
        }
        self.update_module(&module_dir)
    }

    pub fn update_module(&self, module_dir: &Path) -> Result<String> {
        let bundle = self.indexer.bundle_for_module(module_dir, 200)?;
        let tripwires = self.derive_tripwires(module_dir, &bundle)?;
        let postmortems = self.postmortems.for_module(module_dir);

        let last_commit = bundle
            .commits
            .first()
            .map(|c| c.short_sha.clone())
            .unwrap_or_else(|| self.indexer.repo.head_short_sha().unwrap_or_default());

        let fm = MemoirFrontmatter {
            module: module_dir.to_string_lossy().to_string(),
            last_updated: Utc::now().to_rfc3339(),
            last_commit: last_commit.clone(),
            evidence_tier: tier(&bundle, postmortems.len()),
            tripwires: tripwires.clone(),
        };

        let body = render_body(module_dir, &bundle, &postmortems, &tripwires, &last_commit);
        let mut out = String::new();
        out.push_str("---\n");
        out.push_str(&serde_yaml::to_string(&fm)?);
        out.push_str("---\n\n");
        out.push_str(&body);

        // smoke check: parse what we just wrote.
        let (parsed_fm, _) = split_frontmatter(&out)?;
        serde_yaml::from_str::<MemoirFrontmatter>(&parsed_fm)?;

        atomic_write(&self.memoir_path(module_dir), &out)?;
        Ok(out)
    }

    fn derive_tripwires(
        &self,
        module_dir: &Path,
        bundle: &ArtifactBundle,
    ) -> Result<Vec<TripwireSpec>> {
        let incident_re = Regex::new(r"\b([A-Z]{2,4}-\d{2,5})\b").unwrap();

        // collect candidate (incident_id, commit) pairs
        let mut by_incident: BTreeMap<String, Vec<&crate::git::CommitInfo>> = BTreeMap::new();
        for c in &bundle.commits {
            for cap in incident_re.captures_iter(&c.message) {
                let id = cap[1].to_string();
                // Only count it as a fix if there's a matching issue in the
                // tracker fixture OR a postmortem with that id.
                let known = self.indexer.issue(&id).is_some()
                    || self.postmortems.by_id(&id).is_some();
                if known {
                    by_incident.entry(id).or_default().push(c);
                }
            }
        }

        let mut tripwires = Vec::new();
        let mut idx = 1;
        for (incident_id, commits) in by_incident {
            // Pick the commit whose PR introduced a test as the "guarding"
            // commit; otherwise fall back to the most recent commit.
            let pr = commits
                .iter()
                .find_map(|c| self.indexer.pr_for_commit(&c.sha))
                .cloned();
            let guarding_test = pr
                .as_ref()
                .and_then(|p| p.linked_tests.first().cloned())
                .or_else(|| {
                    self.postmortems
                        .by_id(&incident_id)
                        .and_then(|pm| pm.fm.linked_tests.first().cloned())
                })
                .unwrap_or_else(|| format!("tests::{}_regression", incident_id.to_lowercase()));

            // Region: prefer a postmortem `code_refs` entry pointing inside
            // module_dir; fall back to whole-file 1..=1 sentinel.
            let region = self
                .region_for_incident(&incident_id, module_dir, pr.as_ref())
                .unwrap_or_else(|| RegionSpec {
                    file: module_dir.join("retry.py").to_string_lossy().to_string(),
                    lines: [1, 1],
                });

            let summary = format!(
                "Region originally added to close {incident_id}; reverting it re-opens the same failure mode."
            );

            tripwires.push(TripwireSpec {
                id: format!("TW-{idx:03}"),
                region,
                incident: incident_id.clone(),
                guarding_test,
                summary,
            });
            idx += 1;
        }
        Ok(tripwires)
    }

    fn region_for_incident(
        &self,
        incident_id: &str,
        module_dir: &Path,
        _pr: Option<&PullRequest>,
    ) -> Option<RegionSpec> {
        if let Some(pm) = self.postmortems.by_id(incident_id) {
            for r in &pm.current_code_refs {
                let p = Path::new(&r.file);
                if p.starts_with(module_dir) {
                    return Some(RegionSpec {
                        file: r.file.clone(),
                        lines: r.lines,
                    });
                }
            }
        }
        None
    }
}

fn tier(bundle: &ArtifactBundle, postmortems: usize) -> String {
    let c = bundle.commits.len();
    let i = bundle.issues.len() + postmortems;
    let p = bundle.prs.len();
    if c >= 3 && i >= 1 && p >= 1 {
        "strong".into()
    } else if c >= 1 && (i >= 1 || p >= 1) {
        "moderate".into()
    } else {
        "weak".into()
    }
}

fn render_body(
    module_dir: &Path,
    bundle: &ArtifactBundle,
    postmortems: &[&Postmortem],
    tripwires: &[TripwireSpec],
    last_commit: &str,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# {} — Memoir\n", module_dir.display());

    // Evidence summary line
    let _ = writeln!(
        s,
        "_Last updated **{ts}** · `{sha}` · {nc} commits · {ni} incidents · {np} PRs · {nt} guarding {test_word}._\n",
        ts = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        sha = last_commit,
        nc = bundle.commits.len(),
        ni = bundle.issues.len() + postmortems.len(),
        np = bundle.prs.len(),
        nt = tripwires.len(),
        test_word = if tripwires.len() == 1 { "test" } else { "tests" },
    );

    s.push_str("## What this module does\n\n");
    s.push_str(
        "Generated from the repo at memoir-update time. This section is a \
short, evidence-bound description of the module's responsibility. Replace \
this paragraph if a human-authored summary exists at the top of the \
module — Reckon will preserve it on the next regeneration.\n\n",
    );

    s.push_str("## Why it's shaped this way\n\n");
    if bundle.commits.is_empty() {
        s.push_str("_No commits indexed yet._\n\n");
    } else {
        for (i, c) in bundle.commits.iter().take(8).enumerate() {
            let _ = writeln!(
                s,
                "{n}. **{summary}** — commit `{sha}`.[^{n}]",
                n = i + 1,
                summary = c.summary,
                sha = c.short_sha,
            );
        }
        s.push('\n');
    }

    s.push_str("## Scars\n\n");
    if postmortems.is_empty() {
        s.push_str("_No postmortems link to this module._\n\n");
    } else {
        for pm in postmortems {
            let _ = writeln!(
                s,
                "> **{id} · {date} · {title}**\n>\n> {body}\n>\n> See `{src}`.\n",
                id = pm.fm.id,
                date = pm.fm.date,
                title = pm.fm.title,
                body = first_paragraph(&pm.body),
                src = relpath_from_repo(&pm.source_path),
            );
        }
    }

    s.push_str("## Tripwires\n\n");
    if tripwires.is_empty() {
        s.push_str(
            "_No tripwires registered. A tripwire is generated when an indexed \
commit references a known incident **and** the same change ships a test._\n\n",
        );
    } else {
        s.push_str("| ID | Region | Incident | Guarding test |\n");
        s.push_str("|----|--------|----------|---------------|\n");
        for t in tripwires {
            let _ = writeln!(
                s,
                "| {id} | `{file}:{a}–{b}` | {inc} | `{test}` |",
                id = t.id,
                file = t.region.file,
                a = t.region.lines[0],
                b = t.region.lines[1],
                inc = t.incident,
                test = t.guarding_test,
            );
        }
        s.push('\n');
    }

    s.push_str("## Rejected alternatives\n\n");
    let mut rejected = bundle
        .prs
        .iter()
        .filter(|p| p.state == "closed" && p.merged_at.is_none())
        .collect::<Vec<_>>();
    rejected.sort_by(|a, b| a.number.cmp(&b.number));
    if rejected.is_empty() {
        s.push_str("_None recorded._\n\n");
    } else {
        for p in rejected {
            let closed = p.closed_at.clone().unwrap_or_else(|| "unknown".into());
            let _ = writeln!(
                s,
                "- **PR #{n} (closed {closed})** — {title}. {body}",
                n = p.number,
                closed = closed,
                title = p.title,
                body = first_paragraph(&p.body),
            );
        }
        s.push('\n');
    }

    s.push_str("---\n\n");
    let _ = writeln!(
        s,
        "_Auto-maintained by Bob Reckon. Last updated by commit `{last_commit}`._\n"
    );

    // Footnotes
    for (i, c) in bundle.commits.iter().take(8).enumerate() {
        let _ = writeln!(s, "[^{n}]: commit `{sha}` — \"{summary}\"", n = i + 1, sha = c.short_sha, summary = c.summary);
    }

    s
}

fn first_paragraph(s: &str) -> String {
    for para in s.split("\n\n") {
        let trimmed = para.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed.replace('\n', " ");
    }
    String::new()
}

fn relpath_from_repo(p: &Path) -> String {
    p.file_name()
        .map(|s| format!("postmortems/{}", s.to_string_lossy()))
        .unwrap_or_else(|| p.to_string_lossy().to_string())
}
