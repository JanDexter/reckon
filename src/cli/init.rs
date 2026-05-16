//! `reckon init` — scaffold a new repo for Reckon use.
//!
//! Creates:
//!   - `.reckon/` sidecar dir + `.gitignore` entry
//!   - `postmortems/` with a `TEMPLATE.md` for new incidents
//!   - optional `.git/hooks/post-commit` that auto-runs
//!     `reckon memoir` on each touched module after every commit
//!
//! Idempotent: re-running on an already-initialized repo never destroys
//! state; it prints a "nothing to do" line per item that already exists.

use std::fs;
use std::path::Path;

use crate::cli::style::{Style, Theme};

pub struct InitArgs {
    pub with_hooks: bool,
    pub no_color: bool,
    pub force: bool,
}

pub fn run(reckon: &crate::Reckon, args: InitArgs) -> crate::Result<()> {
    let theme = if args.no_color { Theme::plain() } else { Theme::auto() };
    let root = &reckon.repo_root;

    println!(
        "  {} · {}",
        crate::cli::wordmark::standalone(theme),
        Style::default().paint(theme, &format!("initializing in {}", root.display())),
    );
    println!();

    // 1. .reckon/ sidecar dir (Reckon::open creates this, but be explicit).
    let sidecar = root.join(".reckon");
    step(theme, sidecar.exists(), "created", "exists",
        ".reckon/  (sidecar: SQLite + caches)", || {
            fs::create_dir_all(&sidecar)?;
            Ok(())
        })?;

    // 2. .gitignore entry for the sidecar.
    let gitignore = root.join(".gitignore");
    let needle = ".reckon/";
    let existing = fs::read_to_string(&gitignore).unwrap_or_default();
    let already = existing.lines().any(|l| {
        let t = l.trim();
        t == needle || t == "/.reckon/" || t == "/.reckon" || t == "**/.reckon/"
    });
    step(theme, already, "added", "already gitignored",
        ".reckon/  → .gitignore", || {
            let mut out = existing.clone();
            if !out.ends_with('\n') && !out.is_empty() {
                out.push('\n');
            }
            out.push_str("# Reckon sidecar (caches, evidence store).\n");
            out.push_str(".reckon/\n");
            fs::write(&gitignore, out)?;
            Ok(())
        })?;

    // 3. postmortems/ directory + template.
    let pm_dir = root.join("postmortems");
    step(theme, pm_dir.exists(), "created", "exists",
        "postmortems/", || {
            fs::create_dir_all(&pm_dir)?;
            Ok(())
        })?;
    let template = pm_dir.join("TEMPLATE.md");
    step(theme, template.exists() && !args.force, "wrote", "exists",
        "postmortems/TEMPLATE.md", || {
            fs::write(&template, POSTMORTEM_TEMPLATE)?;
            Ok(())
        })?;

    // 4. optional git post-commit hook.
    if args.with_hooks {
        let hooks_dir = root.join(".git").join("hooks");
        if !hooks_dir.exists() {
            return Err(crate::Error::Other(
                "not a git repo (no .git/hooks directory)".into(),
            ));
        }
        let hook_path = hooks_dir.join("post-commit");
        let hook_exists = hook_path.exists();
        let already_wired = hook_exists
            && fs::read_to_string(&hook_path)
                .map(|s| s.contains("reckon memoir"))
                .unwrap_or(false);
        step(theme, already_wired, "wired", "already wired",
            ".git/hooks/post-commit  → reckon memoir --auto", || {
                if hook_exists && !args.force {
                    return Err(crate::Error::Other(
                        format!("{} already exists; pass --force to overwrite", hook_path.display()),
                    ));
                }
                fs::write(&hook_path, POST_COMMIT_HOOK)?;
                make_executable(&hook_path)?;
                Ok(())
            })?;
    }

    println!();
    println!(
        "  {}  {}",
        Style::green().paint(theme, "✓"),
        Style::bold().paint(theme, "ready"),
    );
    println!(
        "     {}",
        Style::dim().paint(
            theme,
            "next: `reckon memoir <module>` to generate the first MEMOIR.md",
        ),
    );
    println!(
        "     {}",
        Style::dim().paint(
            theme,
            "or:   `reckon doctor` to verify the Bob wiring",
        ),
    );
    Ok(())
}

fn step<F>(
    theme: Theme,
    skip: bool,
    verb_done: &str,
    verb_skip: &str,
    label: &str,
    body: F,
) -> crate::Result<()>
where
    F: FnOnce() -> crate::Result<()>,
{
    if skip {
        println!(
            "  {} {}  {}",
            Style::dim().paint(theme, "·"),
            Style::dim().paint(theme, verb_skip),
            Style::dim().paint(theme, label),
        );
        return Ok(());
    }
    body()?;
    println!(
        "  {} {}  {}",
        Style::green().paint(theme, "✓"),
        Style::default().paint(theme, verb_done),
        Style::bold().paint(theme, label),
    );
    Ok(())
}

#[cfg(unix)]
fn make_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(p)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(p, perm)
}

#[cfg(not(unix))]
fn make_executable(_p: &Path) -> std::io::Result<()> {
    Ok(())
}

const POSTMORTEM_TEMPLATE: &str = "---
id: IR-XXX
date: YYYY-MM-DD
title: <one-line incident title>
services:
  - <service-name>
code_refs:
  - { file: \"path/to/file.ext\", lines: [START, END] }
linked_prs:
  - \"#NNNN\"
linked_tests:
  - \"path/to/tests/test_foo.py::test_regression_guard\"
---

# IR-XXX — <title>

## Summary

What happened, in a paragraph. Be specific: which service, which window,
how many customers/requests affected, dollar impact if known.

## Fix

What changed in the code, with a pointer to the PR and the test that
holds the regression. Reckon uses this section to derive the tripwire
summary that surfaces in Bob's Review-mode warning card.

## Why this still matters

Why the future engineer reading this should not undo it. The trap that
re-opens this failure mode, stated plainly.
";

const POST_COMMIT_HOOK: &str = "#!/usr/bin/env sh
# Reckon auto-update hook.
#
# After every commit, regenerate the MEMOIR.md of each module that this
# commit touched. The new memoirs land in the working tree as uncommitted
# changes so you can either amend them in or commit them as a follow-up.
#
# Disable with: `git config reckon.autoUpdateMemoir false`
# Remove entirely by deleting `.git/hooks/post-commit`.

if [ \"$(git config --bool reckon.autoUpdateMemoir 2>/dev/null)\" = \"false\" ]; then
    exit 0
fi
if ! command -v reckon >/dev/null 2>&1; then
    exit 0
fi

git diff-tree --no-commit-id --name-only -r HEAD | awk -F/ '{ \
    if (NF >= 2) { print $1 } \
}' | sort -u | while IFS= read -r module; do
    if [ -d \"$module\" ]; then
        reckon memoir \"$module\" --auto --no-color || true
    fi
done
";
