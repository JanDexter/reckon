//! `reckon install <host>` — atomically merge the `mcpServers.reckon`
//! entry into the host's MCP config, or print the snippet to stdout.
//!
//! Supported hosts (per their documented paths):
//!   - `bob`     → IBM Bob 1.x at `~/.bob/settings/mcp_settings.json`,
//!                 falling back to the docs-canonical `~/.bob/mcp_settings.json`
//!   - `claude`  → Claude Desktop platform-specific path
//!   - `cursor`  → `~/.cursor/mcp.json`
//!   - `print`   → emit the JSON block to stdout (paste into any host)
//!
//! Preserves every other server already in the config. Refuses to
//! clobber an existing `reckon` entry unless `--force` is set.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::cli::style::{Style, Theme};

#[derive(Debug, Clone, Copy)]
pub enum Host {
    Bob,
    Claude,
    Cursor,
    Print,
}

impl std::str::FromStr for Host {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bob" => Ok(Host::Bob),
            "claude" | "claude-desktop" => Ok(Host::Claude),
            "cursor" => Ok(Host::Cursor),
            "print" | "stdout" => Ok(Host::Print),
            other => Err(format!(
                "unknown host '{other}'. Try: bob | claude | cursor | print"
            )),
        }
    }
}

pub struct InstallArgs {
    pub host: Host,
    pub force: bool,
    pub no_color: bool,
    /// Pin to a specific repo instead of `${workspaceFolder}`.
    pub repo_path: Option<PathBuf>,
}

pub fn run(_reckon: &crate::Reckon, args: InstallArgs) -> crate::Result<()> {
    let theme = if args.no_color { Theme::plain() } else { Theme::auto() };

    let block = build_block(args.repo_path.as_deref());

    if matches!(args.host, Host::Print) {
        let snippet = json!({ "mcpServers": { "reckon": block } });
        println!("{}", serde_json::to_string_pretty(&snippet)?);
        return Ok(());
    }

    let target = match args.host {
        Host::Bob => detect_bob_config()?,
        Host::Claude => detect_claude_config()?,
        Host::Cursor => detect_cursor_config()?,
        Host::Print => unreachable!(),
    };

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let merged = merge_into(&target, block, args.force)?;
    fs::write(&target, &merged)?;

    println!(
        "  {} {}  {}",
        Style::green().paint(theme, "✓"),
        Style::default().paint(theme, "wrote"),
        Style::bold().paint(theme, &target.display().to_string()),
    );
    println!(
        "     {}",
        Style::dim().paint(theme, "next: restart your MCP host, then `reckon doctor`"),
    );
    Ok(())
}

fn build_block(repo_path: Option<&Path>) -> Value {
    let repo_value = match repo_path {
        Some(p) => Value::String(p.display().to_string()),
        None => Value::String("${workspaceFolder}".into()),
    };
    json!({
        "command": "reckon-mcp",
        "args": [],
        "env": { "RECKON_REPO": repo_value },
        "description": "Persistent, version-controlled memory for the codebase."
    })
}

fn merge_into(path: &Path, block: Value, force: bool) -> crate::Result<String> {
    let mut root: Value = if path.exists() {
        let raw = fs::read_to_string(path)?;
        if raw.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&raw).map_err(|e| crate::Error::Other(
                format!("existing config at {} is not valid JSON: {e}", path.display())
            ))?
        }
    } else {
        json!({})
    };

    if !root.is_object() {
        return Err(crate::Error::Other(format!(
            "existing config at {} is not a JSON object",
            path.display()
        )));
    }
    let root_obj = root.as_object_mut().unwrap();
    let servers_obj: &mut Map<String, Value> = match root_obj.get_mut("mcpServers") {
        Some(Value::Object(o)) => o,
        Some(_) => {
            return Err(crate::Error::Other(format!(
                "existing `mcpServers` in {} is not an object",
                path.display()
            )));
        }
        None => {
            root_obj.insert("mcpServers".into(), Value::Object(Map::new()));
            root_obj
                .get_mut("mcpServers")
                .and_then(|v| v.as_object_mut())
                .unwrap()
        }
    };

    if servers_obj.contains_key("reckon") && !force {
        return Err(crate::Error::Other(
            "`reckon` server already configured; pass --force to overwrite".into(),
        ));
    }
    servers_obj.insert("reckon".to_string(), block);

    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

// ───────────────────────── host config probes ─────────────────────────

fn detect_bob_config() -> crate::Result<PathBuf> {
    // Bob 1.x writes to `~/.bob/settings/mcp_settings.json` (verified
    // against an active install). The IBM docs name
    // `~/.bob/mcp_settings.json`; honor whichever exists first, then
    // legacy %APPDATA% paths some tutorials reference.
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if let Some(h) = home.as_ref() {
        let v1x = h.join(".bob").join("settings").join("mcp_settings.json");
        if v1x.exists() { return Ok(v1x); }
        let canonical = h.join(".bob").join("mcp_settings.json");
        if canonical.exists() { return Ok(canonical); }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let legacy_a = PathBuf::from(&appdata).join("Bob").join("mcp.json");
        if legacy_a.exists() { return Ok(legacy_a); }
        let legacy_b = PathBuf::from(&appdata).join("IBM").join("Bob").join("mcp.json");
        if legacy_b.exists() { return Ok(legacy_b); }
    }
    // Nothing exists yet — create at the 1.x path, since that's where
    // Bob 1.x actually reads from.
    if let Some(h) = home {
        return Ok(h.join(".bob").join("settings").join("mcp_settings.json"));
    }
    Err(crate::Error::Other(
        "HOME / USERPROFILE not set — cannot locate Bob config".into(),
    ))
}

fn detect_claude_config() -> crate::Result<PathBuf> {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Ok(PathBuf::from(appdata)
                .join("Claude")
                .join("claude_desktop_config.json"));
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json"));
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join("Claude")
            .join("claude_desktop_config.json"));
    }
    Err(crate::Error::Other("cannot locate Claude Desktop config".into()))
}

fn detect_cursor_config() -> crate::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| crate::Error::Other(
            "HOME / USERPROFILE not set — cannot locate Cursor config".into(),
        ))?;
    Ok(home.join(".cursor").join("mcp.json"))
}
