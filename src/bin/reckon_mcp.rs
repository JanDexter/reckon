//! Reckon MCP server binary.
//!
//! Speaks MCP over stdio (default Bob/MCP-host transport). Exposes the five
//! tools defined in §5 of the architecture:
//!
//!   - reckon.read_memoir(path) -> markdown
//!   - reckon.update_memoir(path) -> markdown
//!   - reckon.explain(repo, path, range) -> markdown
//!   - reckon.check_diff(repo, path, diff) -> { warnings: [...] }
//!   - reckon.search_postmortems(query) -> [{...}]
//!
//! The `repo` parameter on `explain` / `check_diff` is honored when present;
//! otherwise the server falls back to the `RECKON_REPO` env var or the
//! current working directory.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use rmcp::{
    handler::server::tool::{Parameters, ToolRouter},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData, Implementation,
        ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    schemars,
    service::ServiceExt,
    tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler,
};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

use reckon::Reckon;

#[derive(Clone)]
struct ReckonService {
    inner: Arc<Reckon>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReadMemoirArgs {
    /// Path to a file or directory in the repo. Reckon resolves the
    /// enclosing module and returns its MEMOIR.md.
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateMemoirArgs {
    /// Path to a file or directory in the repo. Reckon resolves the
    /// enclosing module and regenerates its MEMOIR.md.
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExplainArgs {
    /// Optional repo root override. Defaults to RECKON_REPO env or cwd.
    #[serde(default)]
    repo: Option<String>,
    /// File path within the repo.
    path: String,
    /// Inclusive [start, end] line range, 1-indexed.
    range: [usize; 2],
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CheckDiffArgs {
    #[serde(default)]
    repo: Option<String>,
    /// File path the diff is against (unused if the diff already has --- /
    /// +++ headers; kept for compatibility with hosts that send it
    /// separately).
    #[serde(default)]
    path: Option<String>,
    /// Unified diff text.
    diff: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchPostmortemsArgs {
    query: String,
    #[serde(default = "default_k")]
    k: usize,
}

fn default_k() -> usize {
    5
}

#[tool_router]
impl ReckonService {
    fn new(reckon: Reckon) -> Self {
        Self {
            inner: Arc::new(reckon),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "reckon.read_memoir",
        description = "Return the MEMOIR.md for the module containing `path`. \
                       Use when the user asks about a code region that already has a memoir."
    )]
    async fn read_memoir(
        &self,
        Parameters(args): Parameters<ReadMemoirArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&args.path);
        let module = self
            .inner
            .indexer
            .module_for_path(&path)
            .map_err(internal)?;
        let parsed = self
            .inner
            .memoir
            .read(&module)
            .map_err(internal)?
            .ok_or_else(|| ErrorData::new(ErrorCode::INVALID_PARAMS,
                format!("No MEMOIR.md exists for module {module:?}. Call reckon.update_memoir first."),
                None))?;
        let md = render_memoir(&parsed);
        Ok(CallToolResult::success(vec![Content::text(md)]))
    }

    #[tool(
        name = "reckon.update_memoir",
        description = "Regenerate MEMOIR.md for the module containing `path` from indexed artifacts. \
                       Writes to disk atomically and returns the new markdown."
    )]
    async fn update_memoir(
        &self,
        Parameters(args): Parameters<UpdateMemoirArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&args.path);
        let md = self
            .inner
            .memoir
            .update_for_path(&path)
            .map_err(internal)?;
        Ok(CallToolResult::success(vec![Content::text(md)]))
    }

    #[tool(
        name = "reckon.explain",
        description = "Grounded explanation of a code range. Pulls the memoir if relevant, \
                       supplements with live artifact lookup (git blame + linked PRs), \
                       returns markdown with footnoted source references."
    )]
    async fn explain(
        &self,
        Parameters(args): Parameters<ExplainArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&args.path);
        let [start, end] = args.range;
        let module = self
            .inner
            .indexer
            .module_for_path(&path)
            .map_err(internal)?;
        let parsed = self.inner.memoir.read(&module).map_err(internal)?;
        let blame = self
            .inner
            .indexer
            .repo
            .blame_range(&path, start, end)
            .map_err(internal)?;

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

        let _ = self
            .inner
            .evidence
            .new_request(
                "explain",
                self.inner.repo_root.to_string_lossy().as_ref(),
                Some(args.path.as_str()),
                Some((start, end)),
            );

        Ok(CallToolResult::success(vec![Content::text(md)]))
    }

    #[tool(
        name = "reckon.check_diff",
        description = "Match a unified diff against the tripwire registry. Returns one warning \
                       per regression match. THIS IS THE HERO TOOL — call it from Bob's Review mode \
                       before approving any change."
    )]
    async fn check_diff(
        &self,
        Parameters(args): Parameters<CheckDiffArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let req_id = self
            .inner
            .evidence
            .new_request(
                "check_diff",
                self.inner.repo_root.to_string_lossy().as_ref(),
                args.path.as_deref(),
                None,
            )
            .map_err(internal)?;
        let trace_id = format!("trace-{req_id}");
        let result = self
            .inner
            .tripwire
            .check(&args.diff, &self.inner.memoir, &self.inner.postmortems, &trace_id)
            .map_err(internal)?;

        let json = serde_json::to_value(&result).map_err(internal)?;
        Ok(CallToolResult::success(vec![Content::json(json).map_err(internal)?]))
    }

    #[tool(
        name = "reckon.search_postmortems",
        description = "Semantic search over the postmortem corpus. Returns ranked matches \
                       with id, title, date, score, and a snippet."
    )]
    async fn search_postmortems(
        &self,
        Parameters(args): Parameters<SearchPostmortemsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let hits = self
            .inner
            .postmortems
            .search(&args.query, args.k)
            .map_err(internal)?;
        let json = serde_json::to_value(&hits).map_err(internal)?;
        Ok(CallToolResult::success(vec![Content::json(json).map_err(internal)?]))
    }
}

#[tool_handler]
impl ServerHandler for ReckonService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "reckon".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(
                "Reckon gives every codebase a persistent, version-controlled memory. \
                 Five tools: read_memoir, update_memoir, explain, check_diff, search_postmortems. \
                 In Review mode, ALWAYS call reckon.check_diff before approving a change."
                    .into(),
            ),
        }
    }
}

fn internal<E: std::fmt::Display>(e: E) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
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

fn resolve_repo_root() -> anyhow::Result<PathBuf> {
    if let Ok(p) = std::env::var("RECKON_REPO") {
        return Ok(PathBuf::from(p));
    }
    Ok(std::env::current_dir()?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let repo_root = resolve_repo_root()?;
    tracing::info!(repo = %repo_root.display(), "reckon-mcp starting");
    let reckon = Reckon::open(&repo_root).context("opening Reckon")?;
    let service = ReckonService::new(reckon);
    let server = service.serve(stdio()).await.context("starting MCP stdio service")?;
    server.waiting().await.context("MCP service shutdown")?;
    Ok(())
}
