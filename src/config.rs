use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Per-repo configuration. Lives in `.reckon/config.json` if the user wants
/// to override defaults; otherwise computed from the repo root.
#[derive(Debug, Clone)]
pub struct Config {
    pub repo_root: PathBuf,
    pub sidecar: PathBuf,
    /// Directories that count as a "module" (the smallest unit of memoir).
    /// Defaults to every directory containing at least one tracked source file
    /// plus a sibling `MEMOIR.md` if present.
    pub module_globs: Vec<String>,
    /// Path to postmortems directory, relative to repo root.
    pub postmortems_dir: PathBuf,
    /// Path to the GitHub fixture directory, relative to repo root.
    pub github_fixture_dir: PathBuf,
    /// Embedding model name (fastembed enum lookup).
    pub embedding_model: String,
}

impl Config {
    pub fn for_repo(repo_root: &Path) -> Result<Self> {
        if !repo_root.is_dir() {
            return Err(Error::Config(format!("repo_root {repo_root:?} is not a directory")));
        }
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            sidecar: repo_root.join(".reckon"),
            module_globs: default_module_globs(),
            postmortems_dir: PathBuf::from("postmortems"),
            github_fixture_dir: PathBuf::from(".fixtures/github"),
            embedding_model: "AllMiniLML6V2".to_string(),
        })
    }

    pub fn sidecar_dir(&self) -> &Path {
        &self.sidecar
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.sidecar.join("evidence.db")
    }

    pub fn chroma_dir(&self) -> PathBuf {
        self.sidecar.join("chroma")
    }

    pub fn postmortems_path(&self) -> PathBuf {
        self.repo_root.join(&self.postmortems_dir)
    }

    pub fn github_fixture_path(&self) -> PathBuf {
        self.repo_root.join(&self.github_fixture_dir)
    }
}

fn default_module_globs() -> Vec<String> {
    vec![
        "**/*.py".into(),
        "**/*.rs".into(),
        "**/*.ts".into(),
        "**/*.tsx".into(),
        "**/*.js".into(),
        "**/*.go".into(),
    ]
}
