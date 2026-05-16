//! Postmortem Index.
//!
//! Source: `postmortems/*.md` with YAML frontmatter (`id`, `date`,
//! `services`, `code_refs`). Embedded once at open time using
//! `fastembed` (MiniLM-L6-v2). For the POC the corpus is tiny so we keep
//! the embedding matrix in memory and run brute-force cosine on query.
//!
//! Code-ref resolution: every `code_refs` entry is forwarded through
//! `GitRepo::resolve_code_ref` so even after renames the postmortem still
//! points at the live region.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::indexer::ArtifactIndexer;
use crate::{Error, Result};

pub struct PostmortemIndex {
    pub config: Arc<Config>,
    pub indexer: Arc<ArtifactIndexer>,
    state: OnceLock<IndexState>,
    embeddings: OnceLock<Vec<Vec<f32>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRef {
    pub file: String,
    pub lines: [usize; 2],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostmortemFrontmatter {
    pub id: String,
    pub date: String,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub code_refs: Vec<CodeRef>,
    #[serde(default)]
    pub linked_prs: Vec<String>,
    #[serde(default)]
    pub linked_tests: Vec<String>,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Postmortem {
    pub fm: PostmortemFrontmatter,
    pub body: String,
    /// Resolved against HEAD via the git layer.
    pub current_code_refs: Vec<CodeRef>,
    pub source_path: PathBuf,
}

struct IndexState {
    docs: Vec<Postmortem>,
    by_id: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub date: String,
    pub score: f32,
    pub snippet: String,
    pub source_path: PathBuf,
}

impl PostmortemIndex {
    pub fn open(config: Arc<Config>, indexer: Arc<ArtifactIndexer>) -> Result<Self> {
        Ok(Self {
            config,
            indexer,
            state: OnceLock::new(),
            embeddings: OnceLock::new(),
        })
    }

    /// Corpus + by-id lookup. No network. Built once, cached forever.
    fn state(&self) -> &IndexState {
        self.state.get_or_init(|| {
            self.build_state().unwrap_or(IndexState {
                docs: Vec::new(),
                by_id: HashMap::new(),
            })
        })
    }

    fn build_state(&self) -> Result<IndexState> {
        let docs = load_postmortems(&self.config, &self.indexer)?;
        let mut by_id = HashMap::new();
        for (i, d) in docs.iter().enumerate() {
            by_id.insert(d.fm.id.clone(), i);
        }
        Ok(IndexState { docs, by_id })
    }

    /// Embeddings for the loaded corpus. Lazily computed on first `search`
    /// call so callers that never search (e.g. memoir generation, unit
    /// tests) don't pull the ONNX model.
    fn corpus_embeddings(&self) -> &[Vec<f32>] {
        self.embeddings.get_or_init(|| {
            let docs = &self.state().docs;
            if docs.is_empty() {
                return Vec::new();
            }
            embed_corpus(docs).unwrap_or_default()
        })
    }

    pub fn all(&self) -> &[Postmortem] {
        &self.state().docs
    }

    pub fn by_id(&self, id: &str) -> Option<&Postmortem> {
        let s = self.state();
        s.by_id.get(id).map(|i| &s.docs[*i])
    }

    /// Semantic search across the postmortem corpus. Triggers a one-time
    /// ONNX model download on first invocation.
    pub fn search(&self, query: &str, k: usize) -> Result<Vec<SearchHit>> {
        let s = self.state();
        if s.docs.is_empty() {
            return Ok(Vec::new());
        }
        let embeddings = self.corpus_embeddings();
        if embeddings.is_empty() {
            return Ok(Vec::new());
        }
        let qvec = embed_query(query)?;
        let mut scored: Vec<(f32, usize)> = embeddings
            .iter()
            .enumerate()
            .map(|(i, d)| (cosine(&qvec, d), i))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        Ok(scored
            .into_iter()
            .map(|(score, i)| {
                let d = &s.docs[i];
                SearchHit {
                    id: d.fm.id.clone(),
                    title: d.fm.title.clone(),
                    date: d.fm.date.clone(),
                    score,
                    snippet: snippet(&d.body),
                    source_path: d.source_path.clone(),
                }
            })
            .collect())
    }

    /// All postmortems that mention a path under `module_path` in their
    /// (resolved) `code_refs`. Used by the Memoir Engine to surface scars.
    pub fn for_module(&self, module_path: &std::path::Path) -> Vec<&Postmortem> {
        let prefix = module_path.to_string_lossy();
        self.state()
            .docs
            .iter()
            .filter(|d| {
                d.current_code_refs
                    .iter()
                    .any(|r| r.file.starts_with(prefix.as_ref()))
            })
            .collect()
    }
}

fn load_postmortems(config: &Config, indexer: &ArtifactIndexer) -> Result<Vec<Postmortem>> {
    let dir = config.postmortems_path();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        let (fm, body) = split_frontmatter(&raw)?;
        let fm: PostmortemFrontmatter = if fm.is_empty() {
            PostmortemFrontmatter::default()
        } else {
            serde_yaml::from_str(&fm)?
        };
        let current_code_refs = fm
            .code_refs
            .iter()
            .map(|r| {
                let (np, ns, ne) = indexer
                    .repo
                    .resolve_code_ref(std::path::Path::new(&r.file), r.lines[0], r.lines[1])
                    .unwrap_or_else(|_| (PathBuf::from(&r.file), r.lines[0], r.lines[1]));
                CodeRef {
                    file: np.to_string_lossy().to_string(),
                    lines: [ns, ne],
                }
            })
            .collect();
        out.push(Postmortem {
            fm,
            body: body.to_string(),
            current_code_refs,
            source_path: path,
        });
    }
    Ok(out)
}

pub fn split_frontmatter(input: &str) -> Result<(String, &str)> {
    let trimmed = input.trim_start_matches('\u{feff}');
    if let Some(rest) = trimmed.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            let after = &rest[end + 4..];
            // skip trailing newline after closing ---
            let body = after.trim_start_matches('\n');
            return Ok((fm.to_string(), body));
        }
        return Err(Error::InvalidMemoir(
            "frontmatter opened with --- but never closed".into(),
        ));
    }
    Ok((String::new(), input))
}

fn snippet(body: &str) -> String {
    let s: String = body.chars().take(240).collect();
    if body.chars().count() > 240 {
        format!("{s}…")
    } else {
        s
    }
}

fn embed_corpus(docs: &[Postmortem]) -> Result<Vec<Vec<f32>>> {
    let model = init_model()?;
    let inputs: Vec<String> = docs
        .iter()
        .map(|d| format!("{}\n{}", d.fm.title, d.body))
        .collect();
    model
        .embed(inputs, None)
        .map_err(|e| Error::Embedding(e.to_string()))
}

fn embed_query(query: &str) -> Result<Vec<f32>> {
    let model = init_model()?;
    let mut v = model
        .embed(vec![query.to_string()], None)
        .map_err(|e| Error::Embedding(e.to_string()))?;
    Ok(v.pop().unwrap_or_default())
}

fn init_model() -> Result<TextEmbedding> {
    TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )
    .map_err(|e| Error::Embedding(e.to_string()))
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}
