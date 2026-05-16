//! `reckon` — the top-level CLI binary. Dispatches to subcommands defined
//! under `reckon::cli`.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

use reckon::cli::{check, doctor, init, install, memoir, trace, why};
use reckon::Reckon;

#[derive(Parser)]
#[command(
    name = "reckon",
    version,
    about = "A persistent, version-controlled memory for your codebase.",
    long_about = "reckon answers \"why is this code like this?\", warns when a pending \
                  change reverts a past incident fix, and maintains a MEMOIR.md per \
                  module committed alongside the code.\n\n\
                  The CLI shares its core engine with the Reckon MCP server (reckon-mcp); \
                  running `reckon check` inside Bob's terminal is the canonical flow."
)]
struct Cli {
    /// Override the repository root. Defaults to $RECKON_REPO, then a
    /// walk up from $PWD looking for `.git/`.
    #[arg(long, global = true)]
    repo: Option<PathBuf>,

    /// Disable ANSI colors / styling.
    #[arg(long, global = true, default_value_t = false)]
    no_color: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Explain why a given line range exists.
    Why {
        /// `path/to/file.rs:42` or `path/to/file.rs:42-71`.
        target: String,
        /// Skip the streaming status banner (useful for piped output).
        #[arg(long, default_value_t = false)]
        no_stream: bool,
    },
    /// Match the current staged diff against the tripwire registry.
    Check {
        /// Read the diff from this file instead of running `git diff --cached`.
        #[arg(long)]
        diff: Option<PathBuf>,
        /// Non-interactive output suitable for CI logs.
        #[arg(long, default_value_t = false)]
        ci: bool,
    },
    /// Regenerate the MEMOIR.md for the module containing this path.
    Memoir {
        path: PathBuf,
        /// Force regeneration even when the memoir is up to date.
        #[arg(long, default_value_t = true)]
        regen: bool,
        /// Quieter output suitable for the post-commit hook.
        #[arg(long, default_value_t = false)]
        auto: bool,
    },
    /// Replay the trace recorded for a previous request id.
    Trace { id: String },
    /// Scaffold a new repo for Reckon: `.reckon/`, postmortems/, optional
    /// git post-commit hook that auto-regenerates memoirs.
    Init {
        /// Install `.git/hooks/post-commit` that runs `reckon memoir`
        /// on every touched module after each commit.
        #[arg(long, default_value_t = false)]
        with_hooks: bool,
        /// Overwrite existing files (templates, hooks) instead of
        /// skipping them.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Diagnose the Reckon install: git repo, binary on PATH, memoirs
    /// present, MCP host config detected.
    Doctor,
    /// Atomically merge `mcpServers.reckon` into an MCP host's config.
    Install {
        /// Host: bob | claude | cursor | print.
        host: install::Host,
        /// Overwrite an existing `reckon` entry.
        #[arg(long, default_value_t = false)]
        force: bool,
        /// Pin to a specific repo instead of `${workspaceFolder}`.
        #[arg(long)]
        repo_path: Option<PathBuf>,
    },
    /// Emit a shell completion script.
    Completions {
        /// Target shell.
        shell: Shell,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // `completions` and `install print` don't need an open Reckon.
    if let Cmd::Completions { shell } = cli.cmd {
        let mut cmd = Cli::command();
        let mut buf: Vec<u8> = Vec::new();
        generate(shell, &mut cmd, "reckon", &mut buf);
        std::io::stdout().write_all(&buf)?;
        return Ok(());
    }

    let repo = resolve_repo(cli.repo.as_deref())?;
    let reckon = Reckon::open(&repo).context("opening Reckon")?;

    match cli.cmd {
        Cmd::Why { target, no_stream } => {
            let (path, start, end) = parse_target(&target)?;
            let path = absolutize(&reckon, path);
            why::run(
                &reckon,
                why::WhyArgs {
                    path,
                    start,
                    end,
                    no_color: cli.no_color,
                    no_stream,
                },
            )?;
            Ok(())
        }
        Cmd::Check { diff, ci } => {
            let code = check::run(
                &reckon,
                check::CheckArgs {
                    diff_path: diff,
                    ci,
                    no_color: cli.no_color,
                },
            )?;
            std::process::exit(code);
        }
        Cmd::Memoir { path, regen, auto } => {
            let path = absolutize(&reckon, path);
            memoir::run(
                &reckon,
                memoir::MemoirArgs {
                    path,
                    regen,
                    auto,
                    no_color: cli.no_color,
                },
            )?;
            Ok(())
        }
        Cmd::Trace { id } => {
            trace::run(
                &reckon,
                trace::TraceArgs {
                    id,
                    no_color: cli.no_color,
                },
            )?;
            Ok(())
        }
        Cmd::Init { with_hooks, force } => {
            init::run(
                &reckon,
                init::InitArgs {
                    with_hooks,
                    force,
                    no_color: cli.no_color,
                },
            )?;
            Ok(())
        }
        Cmd::Doctor => {
            let code = doctor::run(
                &reckon,
                doctor::DoctorArgs {
                    no_color: cli.no_color,
                },
            )?;
            std::process::exit(code);
        }
        Cmd::Install { host, force, repo_path } => {
            install::run(
                &reckon,
                install::InstallArgs {
                    host,
                    force,
                    no_color: cli.no_color,
                    repo_path,
                },
            )?;
            Ok(())
        }
        Cmd::Completions { .. } => unreachable!(),
    }
}

fn resolve_repo(explicit: Option<&std::path::Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("RECKON_REPO") {
        return Ok(PathBuf::from(p));
    }
    let cwd = std::env::current_dir()?;
    Ok(reckon::find_repo_root(&cwd).unwrap_or(cwd))
}

fn absolutize(reckon: &Reckon, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        reckon.repo_root.join(path)
    }
}

fn parse_target(s: &str) -> Result<(PathBuf, usize, usize)> {
    // Accept either `path:line` or `path:start-end`.
    let (path, range) = s
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("expected `<path>:<line>` or `<path>:<start>-<end>` but got `{s}`"))?;
    let (start, end) = if let Some((a, b)) = range.split_once('-') {
        let a: usize = a
            .parse()
            .with_context(|| format!("parsing start line in {s:?}"))?;
        let b: usize = b
            .parse()
            .with_context(|| format!("parsing end line in {s:?}"))?;
        (a, b)
    } else {
        let n: usize = range
            .parse()
            .with_context(|| format!("parsing line number in {s:?}"))?;
        (n, n)
    };
    if start == 0 || end < start {
        return Err(anyhow!("invalid line range {start}..{end}"));
    }
    Ok((PathBuf::from(path), start, end))
}
