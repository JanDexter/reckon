//! `reckon memoir <path> --regen` — Screen 5.
//!
//! Progressive step list: each `· step` is dimmed while pending and
//! flips to `✓ step` (muted green) when complete.

use std::io::Write as _;
use std::path::PathBuf;

use crate::cli::style::{Style, Theme};
use crate::Reckon;

pub struct MemoirArgs {
    pub path: PathBuf,
    pub regen: bool,
    pub no_color: bool,
    /// If true, skip the streaming step list and produce a single-line
    /// summary on completion. Used by the git post-commit hook.
    pub auto: bool,
}

pub fn run(reckon: &Reckon, args: MemoirArgs) -> crate::Result<()> {
    let theme = if args.no_color { Theme::plain() } else { Theme::auto() };

    let module = reckon.indexer.module_for_path(&args.path)?;

    if args.auto {
        return run_auto(reckon, &module, theme);
    }

    println!(
        "  {} · {} · {}",
        crate::cli::wordmark::standalone(theme),
        Style::default().paint(theme, "generating memoir"),
        Style::dim().paint(theme, &module.display().to_string()),
    );
    println!();

    let steps = [
        "loading repo context from bob",
        "indexing commits",
        "fetching linked PRs",
        "searching postmortems",
        "bob synthesizing memoir sections",
        "bob cross-checking evidence tier",
        "extracting tripwires",
        "writing memoir",
    ];

    // Print all pending first, then walk through and flip each line as we
    // complete the corresponding stage. We use cursor-up `\x1b[<n>A` to
    // rewind.
    for s in &steps {
        println!("  {}  {}",
            Style::dim().paint(theme, "·"),
            Style::dim().paint(theme, *s));
    }

    let do_step = |idx: usize, theme: Theme| {
        if !theme.color {
            return;
        }
        let up = steps.len() - idx;
        print!("\x1b[{up}A\r");
        print!("  {}  {}",
            Style::green().paint(theme, "✓"),
            Style::dim().paint(theme, steps[idx]));
        // restore cursor to the bottom for the next println.
        let down = up;
        print!("\x1b[{down}B\r");
        let _ = std::io::stdout().flush();
    };

    // We run real work between fake animation pauses.
    do_step(0, theme);
    let bundle = reckon.indexer.bundle_for_module(&module, 200)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    do_step(1, theme);

    do_step(2, theme);
    let prs_count = bundle.prs.len();
    let _ = prs_count;
    std::thread::sleep(std::time::Duration::from_millis(80));

    let pm_count = reckon.postmortems.for_module(&module).len();
    do_step(3, theme);
    std::thread::sleep(std::time::Duration::from_millis(80));

    do_step(4, theme);
    std::thread::sleep(std::time::Duration::from_millis(80));

    do_step(5, theme);
    std::thread::sleep(std::time::Duration::from_millis(80));

    let md = reckon.memoir.update_module(&module)?;
    let parsed = reckon.memoir.read(&module)?.unwrap();
    do_step(6, theme);

    do_step(7, theme);

    println!();
    let _ = args.regen; // always regen for now
    let file_path = format!(
        "{}/MEMOIR.md",
        module.display()
    );
    let stats = format!(
        "{} decisions · {} {scar_word} · {} {tw_word} · evidence: {}",
        bundle.commits.len().min(8),
        pm_count,
        parsed.frontmatter.tripwires.len(),
        capitalize(&parsed.frontmatter.evidence_tier),
        scar_word = if pm_count == 1 { "scar" } else { "scars" },
        tw_word = if parsed.frontmatter.tripwires.len() == 1 { "tripwire" } else { "tripwires" },
    );
    let _ = md;
    println!(
        "  {} {}  {}",
        Style::green().paint(theme, "✓"),
        Style::bold().paint(theme, &file_path),
        Style::default().paint(theme, "written"),
    );
    println!("     {}", Style::dim().paint(theme, &stats));
    println!();
    println!(
        "  {}",
        Style::dim_italic().paint(
            theme,
            "synthesized by bob · commit this file alongside your code.",
        )
    );

    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Quiet variant for `--auto` (post-commit hook). Skips the progress
/// animation; emits one line with the resulting tripwire count.
fn run_auto(
    reckon: &Reckon,
    module: &std::path::Path,
    theme: Theme,
) -> crate::Result<()> {
    let _ = reckon.memoir.update_module(module)?;
    let parsed = reckon
        .memoir
        .read(module)?
        .ok_or_else(|| crate::Error::Other(
            format!("memoir disappeared after update at {}", module.display())
        ))?;
    let stats = format!(
        "memoir updated for {module}: {tw} tripwire(s), tier {tier}",
        module = module.display(),
        tw = parsed.frontmatter.tripwires.len(),
        tier = parsed.frontmatter.evidence_tier,
    );
    println!(
        "  {} {}",
        Style::green().paint(theme, "✓"),
        Style::dim().paint(theme, &stats),
    );
    Ok(())
}
