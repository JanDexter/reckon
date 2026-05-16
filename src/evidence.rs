//! SQLite-backed audit / cache store.
//!
//! Three tables per §8.4 of the architecture:
//!   - `requests(id, kind, repo, path, range, created_at)`
//!   - `sources(request_id, kind, ref, title, snippet)`
//!   - `traces(request_id, step, tool, input, output, latency_ms)`

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::Result;

pub struct EvidenceStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct Source {
    pub kind: String,
    pub r#ref: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct TraceStep {
    pub step: i64,
    pub tool: String,
    pub input: String,
    pub output: String,
    pub latency_ms: i64,
}

impl EvidenceStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Record a tool invocation. Returns the request id.
    pub fn new_request(
        &self,
        kind: &str,
        repo: &str,
        path: Option<&str>,
        range: Option<(usize, usize)>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let range_str = range.map(|(a, b)| format!("{a}-{b}"));
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO requests(kind, repo, path, range, created_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![kind, repo, path, range_str, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn add_source(&self, request_id: i64, src: &Source) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sources(request_id, kind, ref, title, snippet)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![request_id, src.kind, src.r#ref, src.title, src.snippet],
        )?;
        Ok(())
    }

    pub fn add_trace(&self, request_id: i64, step: &TraceStep) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO traces(request_id, step, tool, input, output, latency_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                request_id,
                step.step,
                step.tool,
                step.input,
                step.output,
                step.latency_ms
            ],
        )?;
        Ok(())
    }

    pub fn fetch_traces(&self, request_id: i64) -> Result<Vec<TraceStep>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT step, tool, input, output, latency_ms FROM traces
             WHERE request_id = ?1 ORDER BY step ASC",
        )?;
        let rows = stmt.query_map(params![request_id], |r| {
            Ok(TraceStep {
                step: r.get(0)?,
                tool: r.get(1)?,
                input: r.get(2)?,
                output: r.get(3)?,
                latency_ms: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn fetch_sources(&self, request_id: i64) -> Result<Vec<Source>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT kind, ref, title, snippet FROM sources WHERE request_id = ?1",
        )?;
        let rows = stmt.query_map(params![request_id], |r| {
            Ok(Source {
                kind: r.get(0)?,
                r#ref: r.get(1)?,
                title: r.get(2)?,
                snippet: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS requests (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL,
    repo        TEXT NOT NULL,
    path        TEXT,
    range       TEXT,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sources (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id  INTEGER NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    ref         TEXT NOT NULL,
    title       TEXT NOT NULL,
    snippet     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS traces (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id  INTEGER NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    step        INTEGER NOT NULL,
    tool        TEXT NOT NULL,
    input       TEXT NOT NULL,
    output      TEXT NOT NULL,
    latency_ms  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sources_request_id ON sources(request_id);
CREATE INDEX IF NOT EXISTS idx_traces_request_id ON traces(request_id);
"#;
