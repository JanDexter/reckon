//! Thin wrappers over `git2` for the operations Reckon needs.
//!
//! The underlying `Repository` is not `Sync` (libgit2 is not thread-safe),
//! so we hold it behind a `Mutex` and lock for the duration of each call.
//! Returned values (`CommitInfo`, `BlameLine`, etc.) are owned so callers
//! never see a held lock.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use git2::{BlameOptions, Commit, DiffOptions, Repository, Sort};

use crate::Result;

pub struct GitRepo {
    repo: Mutex<Repository>,
    pub workdir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub author: String,
    pub email: String,
    pub timestamp: i64,
    pub summary: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct BlameLine {
    pub line: usize,
    pub sha: String,
    pub short_sha: String,
    pub summary: String,
    pub author: String,
}

impl GitRepo {
    pub fn open(root: &Path) -> Result<Self> {
        let repo = Repository::discover(root)?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| crate::Error::Other("bare repos are not supported".into()))?
            .to_path_buf();
        Ok(Self {
            repo: Mutex::new(repo),
            workdir,
        })
    }

    pub fn head_short_sha(&self) -> Result<String> {
        let repo = self.repo.lock().unwrap();
        let head = match repo.head() {
            Ok(h) => h,
            Err(_) => return Ok("<unborn>".into()),
        };
        let commit = head.peel_to_commit()?;
        let s = short(commit.id().to_string());
        Ok(s)
    }

    pub fn log_for_path(&self, path: &Path, limit: usize) -> Result<Vec<CommitInfo>> {
        let repo = self.repo.lock().unwrap();
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(Sort::TIME)?;

        let mut out = Vec::with_capacity(limit);
        let path_str = path.to_string_lossy().to_string();
        for oid in revwalk {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            if commit_touches_path(&repo, &commit, &path_str)? {
                out.push(commit_info(&commit));
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let repo = self.repo.lock().unwrap();
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(Sort::TIME)?;
        let mut out = Vec::with_capacity(limit);
        for oid in revwalk {
            let oid = oid?;
            let c = repo.find_commit(oid)?;
            out.push(commit_info(&c));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    pub fn blame_range(&self, path: &Path, start: usize, end: usize) -> Result<Vec<BlameLine>> {
        let repo = self.repo.lock().unwrap();
        let mut opts = BlameOptions::new();
        opts.min_line(start).max_line(end);
        let blame = repo.blame_file(path, Some(&mut opts))?;

        let mut out = Vec::new();
        for line in start..=end {
            if let Some(h) = blame.get_line(line) {
                let oid = h.final_commit_id();
                let commit = repo.find_commit(oid)?;
                out.push(BlameLine {
                    line,
                    sha: oid.to_string(),
                    short_sha: short(oid.to_string()),
                    summary: commit.summary().unwrap_or("").to_string(),
                    author: commit
                        .author()
                        .name()
                        .unwrap_or("(unknown)")
                        .to_string(),
                });
            }
        }
        Ok(out)
    }

    pub fn resolve_code_ref(
        &self,
        original_path: &Path,
        start: usize,
        end: usize,
    ) -> Result<(PathBuf, usize, usize)> {
        let abs = self.workdir.join(original_path);
        if abs.exists() {
            return Ok((original_path.to_path_buf(), start, end));
        }
        let repo = self.repo.lock().unwrap();
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(Sort::TIME)?;
        let path_str = original_path.to_string_lossy().to_string();
        let mut current = original_path.to_path_buf();
        for oid in revwalk {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            if commit.parent_count() == 0 {
                break;
            }
            let parent = commit.parent(0)?;
            let tree_old = parent.tree()?;
            let tree_new = commit.tree()?;
            let mut opts = DiffOptions::new();
            opts.include_typechange(true);
            let mut diff =
                repo.diff_tree_to_tree(Some(&tree_old), Some(&tree_new), Some(&mut opts))?;
            diff.find_similar(None).ok();
            for delta in diff.deltas() {
                let old = delta.old_file().path().unwrap_or(Path::new(""));
                let new = delta.new_file().path().unwrap_or(Path::new(""));
                if old.to_string_lossy() == path_str {
                    current = new.to_path_buf();
                    return Ok((current, start, end));
                }
            }
        }
        Ok((current, start, end))
    }
}

fn commit_info(c: &Commit<'_>) -> CommitInfo {
    let sha = c.id().to_string();
    let short_sha = short(sha.clone());
    let author = c.author();
    CommitInfo {
        sha,
        short_sha,
        author: author.name().unwrap_or("(unknown)").to_string(),
        email: author.email().unwrap_or("").to_string(),
        timestamp: c.time().seconds(),
        summary: c.summary().unwrap_or("").to_string(),
        message: c.message().unwrap_or("").to_string(),
    }
}

fn commit_touches_path(repo: &Repository, commit: &Commit<'_>, path: &str) -> Result<bool> {
    let parent = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };
    let tree = commit.tree()?;
    let mut opts = DiffOptions::new();
    opts.pathspec(path);
    let diff = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut opts))?;
    Ok(diff.deltas().len() > 0)
}

fn short(sha: String) -> String {
    sha.chars().take(7).collect()
}
