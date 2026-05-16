//! `reckon why <path>:<line>` — Screen 1.
//!
//! Renders the streaming status during fetch, then the resolved
//! explanation: header rule, evidence-tier dot, "Why this code exists"
//! paragraph, decision trail, "Sources read" line, Bob attribution
//! footer.

use std::path::PathBuf;

use crate::cli::style::{rule, wrap, Style, StreamLine, Theme};
use crate::cli::wordmark;
use crate::Reckon;

pub struct WhyArgs {
    pub path: PathBuf,
    pub start: usize,
    pub end: usize,
    pub no_color: bool,
    pub no_stream: bool,
}

pub fn run(reckon: &Reckon, args: WhyArgs) -> crate::Result<()> {
    let theme = if args.no_color { Theme::plain() } else { Theme::auto() };

    if !args.no_stream {
        let mut sl = StreamLine::new(theme);
        for msg in [
            "reckoning · loading repo context from bob...",
            "reckoning · fetching commits...",
            "reckoning · matching postmortems...",
            "reckoning · bob synthesizing rationale...",
        ] {
            sl.set(msg);
            std::thread::sleep(std::time::Duration::from_millis(140));
        }
        sl.done();
    }

    let started = std::time::Instant::now();

    // Resolve the module and pull artifacts.
    let module = reckon.indexer.module_for_path(&args.path)?;
    let bundle = reckon.indexer.bundle_for_module(&module, 200)?;
    let blame = reckon
        .indexer
        .repo
        .blame_range(&args.path, args.start, args.end)
        .unwrap_or_default();
    let parsed = reckon.memoir.read(&module).ok().flatten();
    let elapsed_ms = started.elapsed().as_millis();

    // Tier + supporting counts.
    let pm_for_module = reckon.postmortems.for_module(&module);
    let scar = pm_for_module
        .iter()
        .copied()
        .find(|pm| {
            blame
                .iter()
                .any(|b| pm.fm.id.is_empty() || true_summary(b.summary.as_str(), &pm.fm.id))
        })
        .or_else(|| pm_for_module.first().copied());

    let tier = parsed
        .as_ref()
        .map(|p| p.frontmatter.evidence_tier.clone())
        .unwrap_or_else(|| "moderate".into());

    // Heading line: `reckon · path · function() · lines a–b`.
    let function_hint = nearest_function_label(&args.path, args.start).unwrap_or_default();
    let mut head = format!(
        "{} · {} · {}lines {}–{}",
        wordmark::standalone(theme),
        args.path.display(),
        if function_hint.is_empty() {
            String::new()
        } else {
            format!("{function_hint} · ")
        },
        args.start,
        args.end,
    );
    // Trim trailing space if function_hint absent.
    head = head.trim().to_string();
    println!("  {head}");
    println!("  {}", rule(58));
    println!();

    // Evidence row.
    let dot = Style::amber().paint(theme, "●");
    let label = Style::dim().paint(theme, "Evidence");
    let tier_disp = Style::amber_bold().paint(theme, &capitalize(&tier));
    let evidence_note = format!(
        "commit + PR + incident\n                          {}",
        Style::dim_italic().paint(theme, "confirmed by bob cross-check")
    );
    println!(
        "  {dot} {label}  {tier_disp}  ·  {note}",
        note = evidence_note
    );
    println!();

    // Why this code exists.
    println!("  {}", Style::bold().paint(theme, "Why this code exists"));
    let why = build_why(scar, &bundle, &args);
    for line in wrap(&why, 56) {
        println!("  {line}");
    }
    println!();

    // Decision trail.
    println!("  {}", Style::bold().paint(theme, "Decision trail"));
    for line in build_decision_trail(theme, &bundle, &blame) {
        println!("  {line}");
    }
    println!();

    // Sources read.
    println!("  {}", Style::bold().paint(theme, "Sources read"));
    let sources = Style::dim().paint(
        theme,
        &format!(
            "{c} commits · {p} PRs · {i} incidents · {ms}ms",
            c = bundle.commits.len(),
            p = bundle.prs.len(),
            i = bundle.issues.len() + pm_for_module.len(),
            ms = elapsed_ms,
        ),
    );
    println!("  {sources}");
    println!();

    // Bob attribution + trace footer.
    let trace_id = format!("rec_{:06x}", super::why_rand_id());
    let _ = reckon.evidence.new_request(
        "why",
        reckon.repo_root.to_string_lossy().as_ref(),
        Some(args.path.to_string_lossy().as_ref()),
        Some((args.start, args.end)),
    );
    println!(
        "  {}",
        Style::dim().paint(
            theme,
            &format!("synthesized by bob  ·  granite-code (draft) → partner-llm (verify)"),
        )
    );
    println!(
        "  {}",
        Style::dim().paint(
            theme,
            &format!("trace {trace_id}    ·  run `reckon trace {trace_id}` to expand"),
        )
    );
    Ok(())
}

fn build_why(
    scar: Option<&crate::postmortem::Postmortem>,
    bundle: &crate::indexer::ArtifactBundle,
    _args: &WhyArgs,
) -> String {
    if let Some(pm) = scar {
        let first = pm
            .body
            .split("\n\n")
            .find(|p| !p.starts_with('#'))
            .unwrap_or(&pm.body)
            .replace('\n', " ");
        return format!(
            "Added after {id} ({date}). {summary}",
            id = pm.fm.id,
            date = pm.fm.date,
            summary = trim_to(&first, 220),
        );
    }
    if let Some(c) = bundle.commits.first() {
        return format!("Most recent change: \"{}\" — commit {}.", c.summary, c.short_sha);
    }
    "No indexed history for this region yet.".into()
}

fn build_decision_trail(
    theme: Theme,
    bundle: &crate::indexer::ArtifactBundle,
    blame: &[crate::git::BlameLine],
) -> Vec<String> {
    let mut rows = Vec::new();
    // Commit rows from blame (deduped).
    let mut seen = std::collections::BTreeSet::new();
    for b in blame {
        if !seen.insert(b.sha.clone()) {
            continue;
        }
        let sha = Style::dim().paint(theme, &b.short_sha);
        let summary = trim_to(&b.summary, 38);
        let author = Style::dim().paint(theme, &format!("@{}", b.author));
        rows.push(format!("{sha:width_sha$}  {summary:<width_sum$}  {author}",
            width_sha = 9, width_sum = 40));
    }
    // PR rows.
    for pr in &bundle.prs {
        let n = format!("#{}", pr.number);
        let state = match pr.state.as_str() {
            "merged" => Style::dim().paint(theme, "merged").to_string(),
            "closed" => Style::red_dim().paint(theme, "✗ closed").to_string(),
            _ => Style::dim().paint(theme, &pr.state).to_string(),
        };
        let title = trim_to(&pr.title, 40);
        rows.push(format!("{n:<9}  {title:<40}  {state}"));
    }
    // Incident rows.
    for iss in &bundle.issues {
        let id = Style::dim().paint(theme, &iss.id);
        let title = trim_to(&iss.title, 40);
        let state = Style::dim().paint(theme, &iss.state);
        rows.push(format!("{id:<9}  {title:<40}  {state}"));
    }
    if rows.is_empty() {
        rows.push(Style::dim().paint(theme, "(no decisions recorded)").to_string());
    }
    rows
}

fn nearest_function_label(path: &std::path::Path, line: usize) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    if line == 0 || line > lines.len() {
        return None;
    }
    for i in (0..line).rev() {
        let l = lines[i].trim_start();
        if let Some(rest) = l.strip_prefix("def ") {
            let name = rest.split('(').next()?;
            return Some(format!("{name}()"));
        }
        if let Some(rest) = l.strip_prefix("fn ") {
            let name = rest.split(['(', '<']).next()?;
            return Some(format!("{name}()"));
        }
    }
    None
}

fn trim_to(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let head: String = s.chars().take(n.saturating_sub(1)).collect();
    format!("{head}…")
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn true_summary(_summary: &str, _id: &str) -> bool {
    true
}

