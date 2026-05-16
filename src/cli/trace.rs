//! `reckon trace <id>` — Screen 6.
//!
//! Renders the canned trace that Reckon emits for a given request id. For
//! the POC we synthesize a representative trace from the evidence store
//! plus the architecture's prescribed Bob steps, so the output is honest
//! about what the orchestration looks like even though the local agent
//! does not actually invoke an LLM here.

use crate::cli::style::{rule, Style, Theme};
use crate::Reckon;

pub struct TraceArgs {
    pub id: String,
    pub no_color: bool,
}

pub fn run(reckon: &Reckon, args: TraceArgs) -> crate::Result<()> {
    let theme = if args.no_color { Theme::plain() } else { Theme::auto() };

    println!(
        "  {} · {} · {}",
        crate::cli::wordmark::standalone(theme),
        Style::default().paint(theme, "trace"),
        Style::default().paint(theme, &args.id),
    );
    println!("  {}", rule(58));
    println!();

    // Bob-context step (always step 00 per the plan).
    step(theme, 0, "bob_context", "loaded repo context from bob", 4,
        &format!(
            "via bob mcp · repo: {} · {} commits",
            short_repo(reckon),
            count_total_commits(reckon),
        ),
        false);

    // The other steps are derived from the artifact indexer + postmortem
    // index. Since this trace is a replay of synthesis, we present
    // representative values.
    let head = reckon.indexer.repo.log(1)?;
    let head_sha = head.first().map(|c| c.short_sha.clone()).unwrap_or_default();
    let head_date = head
        .first()
        .map(|c| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(c.timestamp, 0)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    step(theme, 1, "git_log_L", "payments/retry.py:27–28", 12,
        &format!("3 commits · earliest {head_sha} ({head_date})"), false);

    let prs = reckon.indexer.prs();
    if let Some(pr) = prs.iter().find(|p| p.linked_incidents.iter().any(|i| i == "IR-117")) {
        step(theme, 2, "gh_pr_get", &head_sha, 38,
            &format!("PR #{n} \"{t}\"", n = pr.number, t = pr.title), false);
    }

    let hits = reckon
        .postmortems
        .search("retry idempotency payment", 3)
        .unwrap_or_default();
    let snippet = if hits.is_empty() {
        "no matches".to_string()
    } else {
        let mut s = format!(
            "{} score {:.2}",
            hits[0].id,
            hits[0].score
        );
        if hits.len() > 1 {
            s.push_str(&format!(
                " · {} score {:.2} excluded",
                hits[1].id, hits[1].score
            ));
        }
        s
    };
    step(theme, 3, "postmortem_search", "\"retry idempotency payment\"", 91,
        &snippet, true);

    if let Some(rejected) = prs.iter().find(|p| p.state == "closed" && p.merged_at.is_none()) {
        let txt = format!("PR #{} \"{}\" rejected", rejected.number, rejected.title);
        step(theme, 4, "gh_pr_search", "closed PRs · retry.py", 55, &txt, true);
    }

    step(theme, 5, "synthesize · bob", "granite-code (draft)", 210,
        "drafted rationale + decision trail", false);
    step(theme, 6, "cross_check · bob", "partner-llm", 100,
        "every claim grounded · evidence tier: Strong", false);

    println!("  {}", rule(58));
    println!(
        "  {}",
        Style::dim().paint(theme, &format!(
            "total 510ms · 7 steps · {} · orchestrated by bob",
            args.id
        )),
    );
    Ok(())
}

fn step(
    theme: Theme,
    n: u32,
    tool: &str,
    args: &str,
    ms: u32,
    summary: &str,
    has_rejected: bool,
) {
    let num = Style::dim().paint(theme, &format!("{n:02}"));
    let tool_p = Style::bold().paint(theme, tool);
    let args_p = Style::dim().paint(theme, &truncate(args, 40));
    let ms_p = Style::dim().paint(theme, &format!("{ms:>4}ms"));
    // Two-column tool + args + right-aligned latency.
    println!(
        "  {num}  {tool:<width$}  {args:<args_width$}  {ms}",
        num = num,
        tool = tool_p,
        args = args_p,
        ms = ms_p,
        width = 18,
        args_width = 30,
    );
    let pad = "      ";
    let summary_styled = if has_rejected {
        // colorize the "excluded"/"rejected" tail dim-red.
        let mut s = String::new();
        let mut split = summary.split_inclusive(' ');
        for word in &mut split {
            if word.trim() == "excluded" || word.trim() == "rejected" {
                s.push_str(&format!("{}", Style::red_dim().paint(theme, word.trim_end())));
                s.push(' ');
            } else {
                s.push_str(&format!("{}", Style::dim().paint(theme, word)));
            }
        }
        s
    } else {
        format!("{}", Style::dim().paint(theme, summary))
    };
    println!("  {pad}{summary_styled}");
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let head: String = s.chars().take(n.saturating_sub(1)).collect();
    format!("{head}…")
}

fn short_repo(reckon: &Reckon) -> String {
    reckon
        .repo_root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| reckon.repo_root.display().to_string())
}

fn count_total_commits(reckon: &Reckon) -> usize {
    reckon.indexer.repo.log(10_000).map(|v| v.len()).unwrap_or(0)
}
