//! Artifact Indexer.
//!
//! Three layers (§8.2):
//!   - Git layer (`git2`)
//!   - Forge layer (GitHub fixture JSON, pluggable for live GitHub)
//!   - Tracker layer (GitHub Issues fixture)
//!
//! Returns structured artifacts the Memoir Engine and Postmortem Index can
//! ingest. Caches parsed fixtures behind `OnceCell` so they are read once
//! per process.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::git::{CommitInfo, GitRepo};
use crate::{Error, Result};

pub struct ArtifactIndexer {
    pub config: Arc<Config>,
    pub repo: GitRepo,
    forge: OnceLock<ForgeBundle>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: String, // "merged" | "closed" | "open"
    pub author: String,
    pub merged_at: Option<String>,
    pub closed_at: Option<String>,
    pub merge_commit: Option<String>,
    pub body: String,
    #[serde(default)]
    pub linked_incidents: Vec<String>,
    #[serde(default)]
    pub linked_tests: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,        // e.g. "IR-117"
    pub number: Option<u64>,
    pub title: String,
    pub state: String,
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ForgeBundle {
    prs: Vec<PullRequest>,
    issues: Vec<Issue>,
    pr_by_commit: HashMap<String, u64>,
    pr_by_number: HashMap<u64, usize>,
    issue_by_id: HashMap<String, usize>,
}

/// Aggregated artifact bundle for a single module.
#[derive(Debug, Clone, Default)]
pub struct ArtifactBundle {
    pub module_path: PathBuf,
    pub commits: Vec<CommitInfo>,
    pub prs: Vec<PullRequest>,
    pub issues: Vec<Issue>,
}

impl ArtifactIndexer {
    pub fn open(repo_root: &Path, config: Arc<Config>) -> Result<Self> {
        Ok(Self {
            repo: GitRepo::open(repo_root)?,
            config,
            forge: OnceLock::new(),
        })
    }

    fn forge(&self) -> &ForgeBundle {
        self.forge.get_or_init(|| load_forge(&self.config).unwrap_or_default())
    }

    pub fn prs(&self) -> &[PullRequest] {
        &self.forge().prs
    }

    pub fn issues(&self) -> &[Issue] {
        &self.forge().issues
    }

    pub fn pr_for_commit(&self, sha: &str) -> Option<&PullRequest> {
        let f = self.forge();
        let short = &sha[..sha.len().min(40)];
        // Match on either full or prefix sha.
        for (commit_sha, pr_no) in &f.pr_by_commit {
            if commit_sha.starts_with(short) || short.starts_with(commit_sha) {
                if let Some(idx) = f.pr_by_number.get(pr_no) {
                    return Some(&f.prs[*idx]);
                }
            }
        }
        None
    }

    pub fn issue(&self, id: &str) -> Option<&Issue> {
        let f = self.forge();
        f.issue_by_id.get(id).map(|i| &f.issues[*i])
    }

    /// Bundle every artifact that mentions `module_dir` (any file under it).
    pub fn bundle_for_module(&self, module_dir: &Path, limit: usize) -> Result<ArtifactBundle> {
        let mut bundle = ArtifactBundle {
            module_path: module_dir.to_path_buf(),
            ..Default::default()
        };

        // Walk every tracked file in the module.
        let mut prs_seen = std::collections::BTreeSet::new();
        let mut issues_seen = std::collections::BTreeSet::new();
        let module_rel = module_dir.to_string_lossy().to_string();

        // Commits: log every commit touching anything under module_dir.
        // `module_dir` is repo-relative; resolve against workdir before
        // walking so the filesystem actually finds the tracked files.
        let module_abs = if module_dir.is_absolute() {
            module_dir.to_path_buf()
        } else {
            self.repo.workdir.join(module_dir)
        };
        let mut commits: Vec<CommitInfo> = Vec::new();
        for entry in walk_files(&module_abs)? {
            let rel = entry
                .strip_prefix(&self.repo.workdir)
                .unwrap_or(&entry)
                .to_path_buf();
            let cs = self.repo.log_for_path(&rel, limit)?;
            for c in cs {
                if !commits.iter().any(|x| x.sha == c.sha) {
                    commits.push(c);
                }
            }
        }
        commits.sort_by_key(|c| std::cmp::Reverse(c.timestamp));

        let incident_re = Regex::new(r"\b([A-Z]{2,4}-\d{2,5})\b").unwrap();
        let pr_re = Regex::new(r"#(\d{1,6})\b").unwrap();

        for c in &commits {
            // Link by merge_commit sha.
            if let Some(pr) = self.pr_for_commit(&c.sha) {
                prs_seen.insert(pr.number);
            }
            // Link by #NNNN reference in commit message.
            for cap in pr_re.captures_iter(&c.message) {
                if let Ok(n) = cap[1].parse::<u64>() {
                    prs_seen.insert(n);
                }
            }
            // Link by incident id reference.
            for cap in incident_re.captures_iter(&c.message) {
                issues_seen.insert(cap[1].to_string());
            }
        }
        bundle.commits = commits;

        // Also bring in PRs whose `files` include the module path.
        for pr in self.prs() {
            if pr.files.iter().any(|f| f.starts_with(&module_rel)) {
                prs_seen.insert(pr.number);
            }
        }

        for n in prs_seen {
            if let Some(idx) = self.forge().pr_by_number.get(&n) {
                let pr = self.forge().prs[*idx].clone();
                for inc in &pr.linked_incidents {
                    issues_seen.insert(inc.clone());
                }
                bundle.prs.push(pr);
            }
        }
        for id in issues_seen {
            if let Some(idx) = self.forge().issue_by_id.get(&id) {
                bundle.issues.push(self.forge().issues[*idx].clone());
            }
        }

        Ok(bundle)
    }

    /// Find the module directory that contains `path`.
    pub fn module_for_path(&self, path: &Path) -> Result<PathBuf> {
        // A "module" is the closest ancestor directory that contains at least
        // one source file alongside it. POC: just use the parent directory.
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.repo.workdir.join(path)
        };
        let parent = abs.parent().ok_or_else(|| Error::ModuleNotFound(path.to_path_buf()))?;
        let rel = parent
            .strip_prefix(&self.repo.workdir)
            .unwrap_or(parent)
            .to_path_buf();
        if rel.as_os_str().is_empty() {
            return Err(Error::ModuleNotFound(path.to_path_buf()));
        }
        Ok(rel)
    }
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for entry in fs::read_dir(&p)? {
                let e = entry?;
                let path = e.path();
                if path.file_name().and_then(|s| s.to_str()) == Some(".git") {
                    continue;
                }
                stack.push(path);
            }
        } else if p.is_file() {
            out.push(p);
        }
    }
    Ok(out)
}

fn load_forge(config: &Config) -> Result<ForgeBundle> {
    let dir = config.github_fixture_path();
    let prs_path = dir.join("prs.json");
    let issues_path = dir.join("issues.json");

    let prs: Vec<PullRequest> = if prs_path.exists() {
        serde_json::from_str(&fs::read_to_string(&prs_path)?)?
    } else {
        Vec::new()
    };
    let issues: Vec<Issue> = if issues_path.exists() {
        serde_json::from_str(&fs::read_to_string(&issues_path)?)?
    } else {
        Vec::new()
    };

    let mut pr_by_commit = HashMap::new();
    let mut pr_by_number = HashMap::new();
    for (i, pr) in prs.iter().enumerate() {
        pr_by_number.insert(pr.number, i);
        if let Some(c) = &pr.merge_commit {
            pr_by_commit.insert(c.clone(), pr.number);
        }
    }
    let mut issue_by_id = HashMap::new();
    for (i, iss) in issues.iter().enumerate() {
        issue_by_id.insert(iss.id.clone(), i);
    }

    Ok(ForgeBundle {
        prs,
        issues,
        pr_by_commit,
        pr_by_number,
        issue_by_id,
    })
}
