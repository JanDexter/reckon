//! Reckon core: memoir engine, artifact indexer, tripwire checker,
//! postmortem index, evidence store, atomic filesystem writer.
//!
//! The MCP server (binary `reckon-mcp`) is a thin shell over [`Reckon`].

pub mod cli;
pub mod config;
pub mod diff;
pub mod error;
pub mod evidence;
pub mod git;
pub mod indexer;
pub mod memoir;
pub mod postmortem;
pub mod tripwire;
pub mod writer;

pub use config::Config;
pub use error::{Error, Result};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use evidence::EvidenceStore;
use indexer::ArtifactIndexer;
use memoir::MemoirEngine;
use postmortem::PostmortemIndex;
use tripwire::TripwireChecker;

/// Top-level facade. One instance per repository.
pub struct Reckon {
    pub config: Arc<Config>,
    pub repo_root: PathBuf,
    pub evidence: Arc<EvidenceStore>,
    pub indexer: Arc<ArtifactIndexer>,
    pub postmortems: Arc<PostmortemIndex>,
    pub memoir: Arc<MemoirEngine>,
    pub tripwire: Arc<TripwireChecker>,
}

impl Reckon {
    /// Open a Reckon instance rooted at `repo_root`. Creates `.reckon/` on
    /// first use; reuses cached SQLite + chroma collections otherwise.
    pub fn open(repo_root: impl AsRef<Path>) -> Result<Self> {
        let raw = repo_root.as_ref();
        if !raw.is_dir() {
            return Err(Error::Config(format!("repo_root {raw:?} is not a directory")));
        }
        // Discover the git repo and adopt its workdir as the canonical
        // root, matching the path shape git2 hands back (no `\\?\`
        // Windows extended prefix). This keeps `strip_prefix` against
        // `workdir` working through every layer.
        let probe = crate::git::GitRepo::open(raw)?;
        let repo_root = probe.workdir.clone();
        drop(probe);
        let config = Arc::new(Config::for_repo(&repo_root)?);

        std::fs::create_dir_all(config.sidecar_dir())?;

        let evidence = Arc::new(EvidenceStore::open(config.sqlite_path())?);
        let indexer = Arc::new(ArtifactIndexer::open(&repo_root, config.clone())?);
        let postmortems = Arc::new(PostmortemIndex::open(config.clone(), indexer.clone())?);
        let memoir = Arc::new(MemoirEngine::new(
            config.clone(),
            indexer.clone(),
            postmortems.clone(),
        ));
        let tripwire = Arc::new(TripwireChecker::new(config.clone()));

        Ok(Self {
            config,
            repo_root,
            evidence,
            indexer,
            postmortems,
            memoir,
            tripwire,
        })
    }
}
