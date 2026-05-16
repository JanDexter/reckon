//! Reckon MCP server binary.
//!
//! Speaks MCP over stdio. Previous revision used the `rmcp` SDK, but its
//! serve loop enforces a strict "client MUST send
//! `notifications/initialized` before any other request" handshake. IBM
//! Bob (and a handful of other hosts) skip that notification and fire
//! `resources/list` immediately after `initialize`, which makes rmcp
//! abort the connection. The MCP spec is on rmcp's side, but the
//! ecosystem in the wild isn't, so this binary hand-rolls the JSON-RPC
//! stdio loop instead.
//!
//! Five tools (§5 of the architecture):
//!   - reckon.read_memoir(path)            -> markdown
//!   - reckon.update_memoir(path)          -> markdown
//!   - reckon.explain(repo, path, range)   -> markdown
//!   - reckon.check_diff(repo, path, diff) -> { warnings: [...] }
//!   - reckon.search_postmortems(query)    -> [hits]
//!
//! Plus six MCP prompts (slash commands) for hosts that surface them:
//! /reckon, /reckon-why, /reckon-check, /reckon-memoir,
//! /reckon-memoir-regen, /reckon-search.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing_subscriber::EnvFilter;

use reckon::Reckon;

const PROTOCOL_VERSION: &str = "2024-11-05";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let repo_root = resolve_repo_root()?;
    tracing::info!(repo = %repo_root.display(), "reckon-mcp starting");
    let reckon = Arc::new(Reckon::open(&repo_root).context("opening Reckon")?);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "bad json on stdin");
                continue;
            }
        };

        // Notifications have no `id` and never receive a response.
        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        if id.is_none() {
            tracing::debug!(method, "notification");
            continue;
        }
        let id = id.unwrap();

        let response = match dispatch(&reckon, &method, params) {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(err) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": err.code,
                    "message": err.message,
                }
            }),
        };

        let serialized = serde_json::to_string(&response)?;
        writeln!(out, "{serialized}")?;
        out.flush()?;
    }

    Ok(())
}

fn resolve_repo_root() -> anyhow::Result<PathBuf> {
    if let Ok(p) = std::env::var("RECKON_REPO") {
        return Ok(PathBuf::from(p));
    }
    let cwd = std::env::current_dir()?;
    Ok(reckon::find_repo_root(&cwd).unwrap_or(cwd))
}

// ───────────────────────── dispatch ─────────────────────────

struct McpError {
    code: i32,
    message: String,
}

impl McpError {
    fn method_not_found(method: &str) -> Self {
        Self { code: -32601, message: format!("method not found: {method}") }
    }
    fn invalid_params(msg: impl Into<String>) -> Self {
        Self { code: -32602, message: msg.into() }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self { code: -32603, message: msg.into() }
    }
}

impl<E: std::fmt::Display> From<E> for McpError {
    fn from(e: E) -> Self {
        McpError::internal(e.to_string())
    }
}

fn dispatch(reckon: &Arc<Reckon>, method: &str, params: Value) -> Result<Value, McpError> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": {},
                "resources": {},
                "prompts": {},
            },
            "serverInfo": {
                "name": "reckon",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": "Reckon gives every codebase a persistent, version-controlled memory. Five tools: read_memoir, update_memoir, explain, check_diff, search_postmortems. In Review mode, ALWAYS call reckon.check_diff before approving a change."
        })),
        "ping" => Ok(json!({})),
        "resources/list" => Ok(json!({ "resources": [] })),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "prompts/list" => Ok(json!({ "prompts": prompt_list() })),
        "prompts/get" => handle_prompt_get(params),
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => handle_tool_call(reckon, params),
        _ => Err(McpError::method_not_found(method)),
    }
}

// ───────────────────────── prompts (slash commands) ─────────────────────────

fn prompt_list() -> Vec<Value> {
    vec![
        json!({
            "name": "reckon",
            "title": "reckon",
            "description": "Show what Reckon knows about a code region. Renders MEMOIR.md + decision trail + tripwire status.",
            "arguments": [
                { "name": "path", "description": "File path within the repo (e.g. payments/retry.py).", "required": true },
                { "name": "lines", "description": "Optional line range (e.g. 27-28 or 42).", "required": false }
            ]
        }),
        json!({
            "name": "reckon-why",
            "title": "reckon why",
            "description": "Grounded \"why is this code like this?\" — pulls memoir, blame, linked PRs.",
            "arguments": [
                { "name": "path", "description": "File path within the repo.", "required": true },
                { "name": "lines", "description": "Line range, e.g. 27-28 or 42.", "required": true }
            ]
        }),
        json!({
            "name": "reckon-check",
            "title": "reckon check",
            "description": "Match the staged diff against the tripwire registry. Hero command in Review mode.",
            "arguments": [
                { "name": "diff", "description": "Unified diff text (paste or supply).", "required": true }
            ]
        }),
        json!({
            "name": "reckon-memoir",
            "title": "reckon memoir",
            "description": "Render the MEMOIR.md for the module containing this path.",
            "arguments": [
                { "name": "path", "description": "File or directory within the repo.", "required": true }
            ]
        }),
        json!({
            "name": "reckon-memoir-regen",
            "title": "reckon memoir --regen",
            "description": "Regenerate the MEMOIR.md from the latest artifacts and write it to disk.",
            "arguments": [
                { "name": "path", "description": "File or directory within the repo.", "required": true }
            ]
        }),
        json!({
            "name": "reckon-search",
            "title": "reckon search-postmortems",
            "description": "Semantic search over the postmortem corpus.",
            "arguments": [
                { "name": "query", "description": "Free-text query.", "required": true }
            ]
        }),
    ]
}

fn handle_prompt_get(params: Value) -> Result<Value, McpError> {
    #[derive(Deserialize)]
    struct GetParams {
        name: String,
        #[serde(default)]
        arguments: std::collections::BTreeMap<String, String>,
    }
    let p: GetParams = serde_json::from_value(params)
        .map_err(|e| McpError::invalid_params(format!("prompts/get params: {e}")))?;
    let arg = |k: &str| p.arguments.get(k).cloned().unwrap_or_default();

    let (description, text) = match p.name.as_str() {
        "reckon" => (
            "Show what Reckon knows about a code region.",
            format!(
                "Use the Reckon MCP server to surface what's known about the code at \
                 `{path}`{lines_clause}. Steps:\n\
                 1. Call `reckon.read_memoir` with `path={path}`. Render the markdown verbatim.\n\
                 2. If the user supplied a line range, call `reckon.explain` with that range to add blame and footnotes.\n\
                 3. Quote any matching Scar verbatim — never paraphrase the memoir.",
                path = arg("path"),
                lines_clause = if arg("lines").is_empty() { String::new() } else { format!(", lines {}", arg("lines")) },
            ),
        ),
        "reckon-why" => (
            "Grounded \"why is this code like this?\"",
            format!(
                "Call `reckon.explain` with `path={path}` and `range=[{a}, {b}]` (parse `{lines}`), \
                 then present the result with the memoir excerpt quoted verbatim and the decision trail as a table. \
                 Cite footnotes back to the underlying commits/PRs.",
                path = arg("path"),
                lines = arg("lines"),
                a = parse_range_start(&arg("lines")),
                b = parse_range_end(&arg("lines")),
            ),
        ),
        "reckon-check" => (
            "Run the staged diff through the tripwire registry.",
            format!(
                "Call `reckon.check_diff` with the diff below. If `warnings.length > 0`, surface each one as a card before doing anything else — quote `memoir_quote` verbatim, show `guarding_test` and `test_assertion`, and offer the actions [Revert this hunk] [Acknowledge and continue] [Show Bob's trace]. If empty, render a single-line confirmation.\n\n\
                 DIFF:\n\
                 ```diff\n{diff}\n```",
                diff = arg("diff"),
            ),
        ),
        "reckon-memoir" => (
            "Render the MEMOIR.md.",
            format!("Call `reckon.read_memoir` with `path={path}` and render the markdown verbatim.", path = arg("path")),
        ),
        "reckon-memoir-regen" => (
            "Regenerate the MEMOIR.md from the latest artifacts.",
            format!(
                "Call `reckon.update_memoir` with `path={path}`. Report which sections changed by diffing the new memoir against the previous one if available.",
                path = arg("path"),
            ),
        ),
        "reckon-search" => (
            "Semantic search over postmortems.",
            format!(
                "Call `reckon.search_postmortems` with `query=\"{q}\"` and render the top results as a ranked list with id, date, score, and snippet.",
                q = arg("query"),
            ),
        ),
        other => return Err(McpError::method_not_found(&format!("prompt {other}"))),
    };

    Ok(json!({
        "description": description,
        "messages": [
            {
                "role": "user",
                "content": { "type": "text", "text": text }
            }
        ]
    }))
}

fn parse_range_start(s: &str) -> String {
    s.split('-').next().unwrap_or(s).trim().to_string()
}

fn parse_range_end(s: &str) -> String {
    let (_a, b) = s.split_once('-').unwrap_or((s, s));
    b.trim().to_string()
}

// ───────────────────────── tool list ─────────────────────────

fn tool_list() -> Vec<Value> {
    vec![
        json!({
            "name": "reckon.read_memoir",
            "description": "Return the MEMOIR.md for the module containing `path`. Use when the user asks about a code region that already has a memoir.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File or directory path within the repo." }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "reckon.update_memoir",
            "description": "Regenerate MEMOIR.md for the module containing `path` from indexed artifacts. Writes to disk atomically and returns the new markdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File or directory path within the repo." }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "reckon.explain",
            "description": "Grounded explanation of a code range. Pulls the memoir if relevant, supplements with live artifact lookup (git blame + linked PRs), returns markdown with footnoted source references.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Optional repo root override." },
                    "path": { "type": "string", "description": "File path within the repo." },
                    "range": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 1 },
                        "minItems": 2, "maxItems": 2,
                        "description": "Inclusive [start, end] line range, 1-indexed."
                    }
                },
                "required": ["path", "range"]
            }
        }),
        json!({
            "name": "reckon.check_diff",
            "description": "Match a unified diff against the tripwire registry. Returns one warning per regression match. THIS IS THE HERO TOOL — call it from Bob's Review mode before approving any change.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo": { "type": "string" },
                    "path": { "type": "string", "description": "Optional file path the diff applies to." },
                    "diff": { "type": "string", "description": "Unified diff text." }
                },
                "required": ["diff"]
            }
        }),
        json!({
            "name": "reckon.search_postmortems",
            "description": "Semantic search over the postmortem corpus. Returns ranked matches with id, title, date, score, and a snippet.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "k": { "type": "integer", "minimum": 1, "default": 5 }
                },
                "required": ["query"]
            }
        }),
    ]
}

// ───────────────────────── tool args ─────────────────────────

#[derive(Deserialize)]
struct ReadMemoirArgs {
    path: String,
}

#[derive(Deserialize)]
struct UpdateMemoirArgs {
    path: String,
}

#[derive(Deserialize)]
struct ExplainArgs {
    #[serde(default, rename = "repo")]
    _repo: Option<String>,
    path: String,
    range: [usize; 2],
}

#[derive(Deserialize)]
struct CheckDiffArgs {
    #[serde(default, rename = "repo")]
    _repo: Option<String>,
    #[serde(default, rename = "path")]
    _path: Option<String>,
    diff: String,
}

#[derive(Deserialize)]
struct SearchPostmortemsArgs {
    query: String,
    #[serde(default = "default_k")]
    k: usize,
}

fn default_k() -> usize {
    5
}

// ───────────────────────── tool dispatch ─────────────────────────

fn handle_tool_call(reckon: &Arc<Reckon>, params: Value) -> Result<Value, McpError> {
    #[derive(Deserialize)]
    struct CallParams {
        name: String,
        #[serde(default)]
        arguments: Value,
    }
    let call: CallParams = serde_json::from_value(params)
        .map_err(|e| McpError::invalid_params(format!("tools/call params: {e}")))?;

    let content = match call.name.as_str() {
        "reckon.read_memoir" | "read_memoir" => read_memoir(reckon, call.arguments)?,
        "reckon.update_memoir" | "update_memoir" => update_memoir(reckon, call.arguments)?,
        "reckon.explain" | "explain" => explain(reckon, call.arguments)?,
        "reckon.check_diff" | "check_diff" => check_diff(reckon, call.arguments)?,
        "reckon.search_postmortems" | "search_postmortems" => {
            search_postmortems(reckon, call.arguments)?
        }
        other => return Err(McpError::method_not_found(&format!("tool {other}"))),
    };

    Ok(json!({ "content": [content], "isError": false }))
}

fn read_memoir(reckon: &Arc<Reckon>, args: Value) -> Result<Value, McpError> {
    let args: ReadMemoirArgs = serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(e.to_string()))?;
    let module = reckon
        .indexer
        .module_for_path(std::path::Path::new(&args.path))
        .map_err(McpError::from)?;
    let parsed = reckon
        .memoir
        .read(&module)
        .map_err(McpError::from)?
        .ok_or_else(|| McpError::invalid_params(format!(
            "No MEMOIR.md exists for module {module:?}. Call reckon.update_memoir first."
        )))?;
    let md = render_memoir(&parsed);
    Ok(text_content(md))
}

fn update_memoir(reckon: &Arc<Reckon>, args: Value) -> Result<Value, McpError> {
    let args: UpdateMemoirArgs = serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(e.to_string()))?;
    let md = reckon
        .memoir
        .update_for_path(std::path::Path::new(&args.path))
        .map_err(McpError::from)?;
    Ok(text_content(md))
}

fn explain(reckon: &Arc<Reckon>, args: Value) -> Result<Value, McpError> {
    let args: ExplainArgs = serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(e.to_string()))?;
    let path = std::path::Path::new(&args.path);
    let [start, end] = args.range;
    let module = reckon.indexer.module_for_path(path).map_err(McpError::from)?;
    let parsed = reckon.memoir.read(&module).map_err(McpError::from)?;
    let blame = reckon
        .indexer
        .repo
        .blame_range(path, start, end)
        .unwrap_or_default();

    let mut md = String::new();
    md.push_str(&format!("# Why `{}:{}–{}`\n\n", path.display(), start, end));
    if let Some(p) = parsed.as_ref() {
        for tw in &p.frontmatter.tripwires {
            if tw.region.lines[0] <= end && tw.region.lines[1] >= start {
                md.push_str(&format!(
                    "**This range is guarded by tripwire `{id}` (incident {inc}).** \
                     The memoir records: _\"{summary}\"_ See the Scars section of \
                     [{module}/MEMOIR.md](./{module}/MEMOIR.md).\n\n",
                    id = tw.id,
                    inc = tw.incident,
                    summary = tw.summary,
                    module = p.frontmatter.module,
                ));
            }
        }
    }
    md.push_str("## Blame\n\n");
    for b in &blame {
        md.push_str(&format!(
            "- L{ln}: `{sha}` — {author}: *{summary}*\n",
            ln = b.line,
            sha = b.short_sha,
            author = b.author,
            summary = b.summary,
        ));
    }
    md.push('\n');

    if let Some(p) = parsed.as_ref() {
        md.push_str("## Memoir excerpt\n\n");
        md.push_str(&truncate_paragraph(&p.body, 800));
        md.push_str("\n\n");
    }

    let _ = reckon.evidence.new_request(
        "explain",
        reckon.repo_root.to_string_lossy().as_ref(),
        Some(args.path.as_str()),
        Some((start, end)),
    );

    Ok(text_content(md))
}

fn check_diff(reckon: &Arc<Reckon>, args: Value) -> Result<Value, McpError> {
    let args: CheckDiffArgs = serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(e.to_string()))?;
    let req_id = reckon
        .evidence
        .new_request(
            "check_diff",
            reckon.repo_root.to_string_lossy().as_ref(),
            None,
            None,
        )
        .map_err(McpError::from)?;
    let trace_id = format!("trace-{req_id}");
    let result = reckon
        .tripwire
        .check(&args.diff, &reckon.memoir, &reckon.postmortems, &trace_id)
        .map_err(McpError::from)?;
    let json = serde_json::to_value(&result).map_err(McpError::from)?;
    Ok(json!({
        "type": "text",
        "text": serde_json::to_string(&json).unwrap_or_default(),
    }))
}

fn search_postmortems(reckon: &Arc<Reckon>, args: Value) -> Result<Value, McpError> {
    let args: SearchPostmortemsArgs = serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(e.to_string()))?;
    let hits = reckon
        .postmortems
        .search(&args.query, args.k)
        .map_err(McpError::from)?;
    let json = serde_json::to_value(&hits).map_err(McpError::from)?;
    Ok(json!({ "type": "text", "text": serde_json::to_string(&json).unwrap_or_default() }))
}

fn text_content(s: impl Into<String>) -> Value {
    json!({ "type": "text", "text": s.into() })
}

fn render_memoir(parsed: &reckon::memoir::ParsedMemoir) -> String {
    let mut s = String::new();
    s.push_str("---\n");
    if let Ok(fm) = serde_yaml::to_string(&parsed.frontmatter) {
        s.push_str(&fm);
    }
    s.push_str("---\n\n");
    s.push_str(&parsed.body);
    s
}

fn truncate_paragraph(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let head: String = s.chars().take(n).collect();
    format!("{head}…")
}
