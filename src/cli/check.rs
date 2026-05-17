//! `reckon check` — Screens 2/3.
//!
//! Reads a unified diff. Sources, in order:
//!   1. `--diff <path>` flag (file to read)
//!   2. stdin (if not a TTY — i.e. piped in)
//!   3. `git diff --cached` from the configured repo
//!
//! No tripwires → Screen 2 (one-line success).
//! At least one tripwire → Screen 3 (warning block + actions row).
//!
//! Honors `--ci` for the non-interactive variant: warning block, single
//! exit line, exit code 1.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::Command;

use crate::cli::style::{rule, warning_box, wrap, Style, Theme};
use crate::cli::wordmark;
use crate::tripwire::Warning;
use crate::Reckon;

pub struct CheckArgs {
    pub diff_path: Option<PathBuf>,
    pub ci: bool,
    pub no_color: bool,
}

pub fn run(reckon: &Reckon, args: CheckArgs) -> crate::Result<i32> {
    let theme = if args.no_color { Theme::plain() } else { Theme::auto() };

    let diff_text = load_diff(reckon, args.diff_path.as_deref())?;
    if diff_text.trim().is_empty() {
        // No staged changes — treat as the happy path.
        return Ok(render_clear(theme));
    }

    let trace_id = format!("rec_{:06x}", super::why_rand_id());
    let result = reckon.tripwire.check(
        &diff_text,
        &reckon.memoir,
        &reckon.postmortems,
        &trace_id,
    )?;

    if result.warnings.is_empty() {
        return Ok(render_clear(theme));
    }

    if args.ci {
        render_warnings_ci(theme, &result.warnings);
        return Ok(1);
    }

    render_warnings_interactive(theme, &result.warnings);
    Ok(1)
}

fn load_diff(reckon: &Reckon, explicit: Option<&std::path::Path>) -> crate::Result<String> {
    if let Some(p) = explicit {
        return Ok(std::fs::read_to_string(p)?);
    }
    // stdin if not a TTY. Heuristic: try to read with a small timeout.
    // For POC, only read stdin if RECKON_STDIN_DIFF=1 to avoid blocking.
    if std::env::var_os("RECKON_STDIN_DIFF").is_some() {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        return Ok(s);
    }
    // Fall back to `git diff --cached` run in the configured repo.
    let out = Command::new("git")
        .arg("-C")
        .arg(&reckon.repo_root)
        .args(["diff", "--cached", "--no-color"])
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => Ok(String::new()),
    }
}

fn render_clear(theme: Theme) -> i32 {
    let check = Style::green().paint(theme, "✓");
    let head = wordmark::standalone(theme);
    let msg = Style::default().paint(theme, "no tripwires triggered");
    println!("  {check} {head}  {msg}");
    0
}

fn render_warnings_interactive(theme: Theme, warnings: &[Warning]) {
    println!(
        "  {}  {} · {}",
        Style::amber_bold().paint(theme, "⚠"),
        wordmark::standalone(theme),
        Style::amber_bold().paint(theme, "tripwire triggered"),
    );
    println!("  {}", rule(58));
    println!();

    for w in warnings {
        render_one(theme, w);
    }

    println!("  {}", rule(58));
    print!(
        "  {}    {}    {} ",
        Style::bold().paint(theme, "[r] revert this hunk"),
        Style::default().paint(theme, "[a] acknowledge and continue"),
        Style::dim().paint(theme, "[?] trace"),
    );
    io::stdout().flush().unwrap();

    // Read user input
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let choice = input.trim().to_lowercase();
        match choice.as_str() {
            "r" => {
                println!("  {} Reverting hunk...", Style::dim().paint(theme, "→"));
                println!("  {} Feature not yet implemented", Style::amber_bold().paint(theme, "⚠"));
            }
            "a" => {
                println!("  {} Acknowledged. Continuing...", Style::dim().paint(theme, "→"));
            }
            "?" => {
                println!("  {} Trace feature not yet implemented", Style::dim().paint(theme, "→"));
            }
            _ => {
                println!("  {} Invalid choice. Please use 'r', 'a', or '?'", Style::amber_bold().paint(theme, "⚠"));
            }
        }
    }
}

fn render_warnings_ci(theme: Theme, warnings: &[Warning]) {
    for w in warnings {
        render_one(theme, w);
    }
    let ids = warnings
        .iter()
        .map(|w| w.incident.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "  {} {} · {} · {} · exiting with code 1",
        Style::amber_bold().paint(theme, "⚠"),
        wordmark::standalone(theme),
        Style::amber_bold().paint(theme, "tripwire triggered"),
        Style::default().paint(theme, &ids),
    );
}

fn render_one(theme: Theme, w: &Warning) {
    let file = &w.file;
    let [s, e] = w.removed_range;
    let intro = format!(
        "Your staged diff removes lines {s}–{e} of {file}.\n  This region is protected by a past incident fix."
    );
    for line in intro.lines() {
        println!("  {line}");
    }
    println!();

    // Warning box: title from incident + memoir quote in body.
    let title = format!("{} · {}", w.incident, summarize_quote(&w.memoir_quote, 56));
    let body_lines = wrap(&w.memoir_quote, 54);
    let mut body = body_lines;
    body.push(String::new());
    body.push(format!("See: postmortems/{}.md", w.incident));

    print!("{}", warning_box(theme, &title, &body));

    println!();
    println!("  {}", Style::dim().paint(theme, "Guarding test"));
    println!("  {}", Style::bold().paint(theme, &w.guarding_test));
    println!("  {}", Style::default().paint(theme, &w.test_assertion));
    println!();

    let dim_label = |s: &str| Style::dim().paint(theme, s).to_string();
    let dim_val = |s: &str| Style::dim().paint(theme, s).to_string();
    println!(
        "  {label}   {val} · tripwire {tw}",
        label = dim_label("memoir"),
        val = dim_val(&format!("{}", "MEMOIR.md")),
        tw = dim_val(&w.tripwire_id),
    );
    println!(
        "  {label}  {val}",
        label = dim_label("context"),
        val = dim_val("loaded from bob · synthesized by bob"),
    );
    println!();
}

fn summarize_quote(s: &str, max: usize) -> String {
    let first = s.split('—').next().unwrap_or(s).trim();
    if first.chars().count() <= max {
        first.to_string()
    } else {
        let head: String = first.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
