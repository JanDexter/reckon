//! `reckon doctor` — diagnose the Reckon install.
//!
//! Runs a series of preflight checks and prints a row per check. Exit
//! code is 0 only if every check passes (warnings are still 0).
//!
//! Checks:
//!   1. cwd resolves to a git repo
//!   2. `.reckon/` sidecar exists
//!   3. `reckon-mcp` binary is on PATH (so Bob can spawn it)
//!   4. at least one MEMOIR.md exists under the repo
//!   5. an MCP host config file is present in a known location

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::style::{Style, Theme};
use crate::Reckon;

pub struct DoctorArgs {
    pub no_color: bool,
}

#[derive(Clone, Copy)]
enum Verdict {
    Ok,
    Warn,
    Fail,
}

pub fn run(reckon: &Reckon, args: DoctorArgs) -> crate::Result<i32> {
    let theme = if args.no_color { Theme::plain() } else { Theme::auto() };
    let root = &reckon.repo_root;

    println!(
        "  {} · {}",
        crate::cli::wordmark::standalone(theme),
        Style::default().paint(theme, "doctor"),
    );
    println!();

    let mut any_fail = false;
    let mut any_warn = false;

    // 1. git repo
    let v = if root.join(".git").exists() { Verdict::Ok } else { Verdict::Fail };
    row(theme, v, "git repository", &format!("{}", root.display()),
        "Reckon needs a git repo to read commits + blame. Run `git init` here.");
    if matches!(v, Verdict::Fail) { any_fail = true; }

    // 2. .reckon/ sidecar
    let sidecar = root.join(".reckon");
    let v = if sidecar.exists() { Verdict::Ok } else { Verdict::Warn };
    row(theme, v, ".reckon/ sidecar",
        if sidecar.exists() { "present" } else { "missing" },
        "Created automatically on first run, or run `reckon init`.");
    if matches!(v, Verdict::Warn) { any_warn = true; }

    // 3. reckon-mcp on PATH
    let mcp_on_path = which_in_path("reckon-mcp");
    let v = if mcp_on_path.is_some() { Verdict::Ok } else { Verdict::Fail };
    row(theme, v, "reckon-mcp on PATH",
        mcp_on_path.as_ref().map(|p| p.display().to_string()).as_deref().unwrap_or("not found"),
        "Run `cargo install --path .` from the Reckon source dir.");
    if matches!(v, Verdict::Fail) { any_fail = true; }

    // 4. at least one MEMOIR.md
    let memoirs = find_memoirs(root);
    let v = if !memoirs.is_empty() { Verdict::Ok } else { Verdict::Warn };
    let memoir_label = if memoirs.is_empty() {
        "none yet".to_string()
    } else {
        format!("{} found", memoirs.len())
    };
    row(theme, v, "MEMOIR.md present", &memoir_label,
        "Run `reckon memoir <module>` to generate the first one.");
    if matches!(v, Verdict::Warn) { any_warn = true; }

    // 5. MCP host config
    let host = detect_mcp_host_config();
    let v = if host.is_some() { Verdict::Ok } else { Verdict::Warn };
    let host_label = host
        .as_ref()
        .map(|h| format!("{} ({})", h.name, h.path.display()))
        .unwrap_or_else(|| "not detected".to_string());
    row(theme, v, "MCP host config", &host_label,
        "Run `reckon install bob` (or claude / cursor) to wire it up.");
    if matches!(v, Verdict::Warn) { any_warn = true; }

    println!();
    if any_fail {
        println!(
            "  {} {}",
            Style::amber_bold().paint(theme, "✗"),
            Style::bold().paint(theme, "failing checks above — Reckon won't work until fixed"),
        );
        return Ok(1);
    }
    if any_warn {
        println!(
            "  {} {}",
            Style::amber().paint(theme, "·"),
            Style::default().paint(theme, "ready, with notes"),
        );
        return Ok(0);
    }
    println!(
        "  {} {}",
        Style::green().paint(theme, "✓"),
        Style::bold().paint(theme, "all systems go"),
    );
    Ok(0)
}

fn row(theme: Theme, v: Verdict, label: &str, value: &str, fix: &str) {
    let mark = match v {
        Verdict::Ok => Style::green().paint(theme, "✓").to_string(),
        Verdict::Warn => Style::amber().paint(theme, "·").to_string(),
        Verdict::Fail => Style::amber_bold().paint(theme, "✗").to_string(),
    };
    let label = format!("{label:<22}");
    let value_styled = match v {
        Verdict::Ok => Style::default().paint(theme, value),
        Verdict::Warn => Style::dim().paint(theme, value),
        Verdict::Fail => Style::amber().paint(theme, value),
    };
    println!("  {mark} {label}  {value_styled}");
    if !matches!(v, Verdict::Ok) {
        println!("       {}", Style::dim().paint(theme, &format!("→ {fix}")));
    }
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let cmd = if cfg!(windows) { "where" } else { "which" };
    let out = Command::new(cmd).arg(name).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    Some(PathBuf::from(line))
}

fn find_memoirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(name, ".git" | "target" | ".reckon" | "node_modules") {
            continue;
        }
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    stack.push(e.path());
                }
            }
        } else if name == "MEMOIR.md" {
            out.push(p);
        }
    }
    out
}

struct HostConfig {
    name: &'static str,
    path: PathBuf,
}

fn detect_mcp_host_config() -> Option<HostConfig> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from));
    let cwd = std::env::current_dir().ok();

    let candidates: Vec<(&'static str, PathBuf)> = [
        // Bob — Bob 1.x actually reads from settings/ subdir.
        home.as_ref().map(|d| ("IBM Bob (1.x)", d.join(".bob").join("settings").join("mcp_settings.json"))),
        // Bob — documented canonical path.
        home.as_ref().map(|d| ("IBM Bob (global)", d.join(".bob").join("mcp_settings.json"))),
        // Bob — project-level.
        cwd.as_ref().map(|d| ("IBM Bob (project)", d.join(".bob").join("mcp.json"))),
        // Bob — legacy.
        appdata.as_ref().map(|d| ("IBM Bob (legacy)", d.join("Bob").join("mcp.json"))),
        // Claude Desktop
        appdata.as_ref().map(|d| ("Claude Desktop", d.join("Claude").join("claude_desktop_config.json"))),
        home.as_ref().map(|d| ("Claude Desktop", d.join(".config").join("Claude").join("claude_desktop_config.json"))),
        home.as_ref().map(|d| ("Claude Desktop", d.join("Library").join("Application Support").join("Claude").join("claude_desktop_config.json"))),
        // Cursor
        home.as_ref().map(|d| ("Cursor", d.join(".cursor").join("mcp.json"))),
    ]
    .into_iter()
    .flatten()
    .collect();

    for (name, path) in candidates {
        if path.exists() {
            return Some(HostConfig { name, path });
        }
    }
    None
}
