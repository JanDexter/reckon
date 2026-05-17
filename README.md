# reckon

> A persistent, version-controlled memory for your codebase.
>
> Auto-generates a `MEMOIR.md` per module, intercepts regressions in
> Bob's Review mode, and answers *"why is this code like this?"* — all
> grounded in commits, PRs, tickets, and postmortems.

Reckon is a Rust CLI and an IBM Bob MCP server sharing a single core.
The CLI surfaces what Bob's agent has reasoned about; Bob's multi-model
orchestration produces the rationale, runs the cross-check, and
downgrades evidence tiers. BobShell is the integrated surface — the
same `reckon check` runs there, blocking commits before they leave the
IDE.

Reckon ships as:

- **`reckon`** — CLI with eight subcommands (`why`, `check`, `memoir`,
  `trace`, `init`, `doctor`, `install`, `completions`).
- **`reckon-mcp`** — MCP server exposing five tools and six prompts
  (slash commands) over stdio.
- **`reckon-seed`** — seeder that builds the synthetic demo
  repository (commits, PRs, issues, postmortems, tests) used to
  rehearse the 90-second demo.

> **Bob is the IDE and agent. Reckon is a capability Bob calls.**

---

## Quick start

```sh
# 1. install the three binaries onto your PATH
cargo install --path .

# 2. wire Reckon into your MCP host
reckon install bob          # or: claude, cursor, print

# 3. inside your own repo
cd /path/to/your/repo
reckon init --with-hooks    # .reckon/ + postmortems/ + post-commit hook
reckon doctor               # verify wiring (5 preflight checks)
reckon memoir src/          # generate the first MEMOIR.md
reckon check                # tripwire-check staged diff
```

`reckon` auto-discovers the repo root by walking up from `$PWD` for a
`.git/` directory — no `RECKON_REPO` env var needed once inside the
repo. Override with `--repo <dir>`.

---

## CLI reference

| Command                                       | Purpose                                                                      |
|-----------------------------------------------|------------------------------------------------------------------------------|
| `reckon init [--with-hooks] [--force]`        | Scaffold `.reckon/` + `postmortems/TEMPLATE.md` + optional post-commit hook. |
| `reckon doctor`                               | Diagnose: git repo, `reckon-mcp` on PATH, memoirs present, host detected.    |
| `reckon install <bob\|claude\|cursor\|print>` | Atomically merge `mcpServers.reckon` into the host's MCP config.             |
| `reckon memoir <path> [--auto]`               | Regenerate the module's `MEMOIR.md`. `--auto` is the quiet hook mode.        |
| `reckon why <path>:<line[-end]>`              | Grounded "why is this here?" with decision trail + Bob attribution.          |
| `reckon check [--diff <file>] [--ci]`         | Match staged diff against tripwires. Hero command.                           |
| `reckon trace <id>`                           | Replay the audit trail of a prior request.                                   |
| `reckon completions <shell>`                  | Emit a shell completion script (`bash` / `zsh` / `fish` / `powershell`).     |

Global flags: `--repo <dir>` (override repo auto-discovery), `--no-color`.

### Shell completions

```sh
reckon completions bash       > /etc/bash_completion.d/reckon
reckon completions zsh        > /usr/local/share/zsh/site-functions/_reckon
reckon completions fish       > ~/.config/fish/completions/reckon.fish
reckon completions powershell | Out-String | Invoke-Expression
```

---

## Demo (90 seconds, against the synthetic IR-117 fixture)

```sh
reckon-seed --out demo-repo --force
cd demo-repo
reckon memoir payments/retry.py
reckon why    payments/retry.py:27-28
reckon check --diff demo-diff.patch
```

The third command — `reckon check` — is the hero. The patch removes the
persisted idempotency key on lines 27–28. Reckon matches the region
against the `MEMOIR.md` tripwire for IR-117 and renders the warning
block.

Inside Bob, the same flow is invoked automatically when the user is in
the `Reckon Review` mode (see "Wiring Reckon into Bob" below).

---

## MCP tools (the contract with Bob)

All tools return markdown when their output is meant to land in Bob's
chat. Structured tools return JSON.

| Tool                                | Returns        | Purpose                                                            |
|-------------------------------------|----------------|--------------------------------------------------------------------|
| `reckon.read_memoir(path)`          | markdown       | Return the MEMOIR.md for the module containing `path`.             |
| `reckon.update_memoir(path)`        | markdown       | Regenerate the memoir from the latest artifacts. Writes to disk.   |
| `reckon.explain(repo,path,range)`   | markdown       | Grounded explanation of a code range. Memoir + blame + linked PRs. |
| `reckon.check_diff(repo,path,diff)` | `{ warnings }` | Match a unified diff against the tripwire registry. **Hero tool.** |
| `reckon.search_postmortems(query)`  | `[ hits ]`     | Semantic search over the postmortem corpus.                        |

### MCP prompts (slash commands)

For hosts that surface MCP prompts in their slash menu, the server also
publishes:

| Slash command            | Calls                                          |
|--------------------------|------------------------------------------------|
| `/reckon`                | `reckon.read_memoir` + optional `reckon.explain` |
| `/reckon-why`            | `reckon.explain`                               |
| `/reckon-check`          | `reckon.check_diff`                            |
| `/reckon-memoir`         | `reckon.read_memoir`                           |
| `/reckon-memoir-regen`   | `reckon.update_memoir`                         |
| `/reckon-search`         | `reckon.search_postmortems`                    |

IBM Bob's documented MCP surface uses chat + the approval workflow
rather than slash; the prompts are still useful for Claude Desktop,
Cursor, and any other host that exposes them.

---

## Bob capability mapping

Reckon flows through Bob's native capabilities at every step.

| Feature                                            | Bob capability used                                |
|----------------------------------------------------|----------------------------------------------------|
| Domain tools (`explain`, `check_diff`, etc.)       | Bob's MCP tool calling                             |
| Synthesis of artifact bundles into rationale       | Bob's multi-LLM routing (Granite + partner models) |
| Trace / audit shown to the user                    | Bob's native audit log                             |
| Review-mode regression interception                | Bob Modes + MCP tool hooks                         |
| Conversational follow-up grounded in same context  | Bob's agent thread + MCP retrieval                 |
| Repo-wide context for memoir generation            | Bob's complete-repo context capability             |

---

## Wiring Reckon into Bob

The fastest path is `reckon install bob`, which writes the JSON snippet
below into Bob's MCP settings. Bob 1.x reads from
`~/.bob/settings/mcp_settings.json` (the docs name
`~/.bob/mcp_settings.json` — `reckon install bob` picks the right path
automatically and falls back to legacy locations if either exists).

```json
{
  "mcpServers": {
    "reckon": {
      "command": "reckon-mcp",
      "args": [],
      "env": { "RECKON_REPO": "${workspaceFolder}" },
      "description": "Persistent, version-controlled memory for the codebase."
    }
  }
}
```

Then restart Bob. Verify with `reckon doctor` — the `MCP host config`
row should read `IBM Bob (1.x) (…/.bob/settings/mcp_settings.json)`.

### Reckon Review (Bob Mode)

Reckon ships best as a Bob **Mode** — a persona that always consults
the memoir before approving a change. To create it:

1. In Bob, open **Settings → Modes → +** (or duplicate `Advanced`).
2. **Slug**: `reckon-review` (avoid `reckon` to prevent conflict with the `/reckon` MCP prompt). **Name**: `Reckon Review`.
3. **Role definition** — paste `bob/prompts/reckon-review-mode.md`
   (this is the persona).
4. **Custom instructions** — paste the procedural workflow rules from
   the same file's "Custom instructions" section.
5. **Available Tools** — check **Read files**, **Edit files**,
   **Execute commands**, **Use MCP**, **Switch modes**.
6. Save and switch to the new mode in the chat picker before reviewing
   any staged diff.

In this mode Bob calls `reckon.check_diff` on every staged hunk before
rendering its own analysis.

---

## Memoir shape

A `MEMOIR.md` is committed to git next to the module it describes. The
frontmatter is the **machine-readable** layer: `tripwires` live here —
that's what `reckon.check_diff` matches against. The body is the
**human-readable** layer: prose, evidence-bound, with footnoted
references back to commits and PRs.

```yaml
---
module: payments
last_updated: 2026-05-14T08:12:00Z
last_commit: a8f3c91
evidence_tier: strong
tripwires:
  - id: TW-001
    region: { file: "payments/retry.py", lines: [27, 28] }
    incident: IR-117
    guarding_test: "tests/payments/test_retry.py::test_idempotency_key_is_deterministic_per_attempt"
    summary: "Removing the persisted idempotency key re-opens IR-117."
---

# payments — Memoir

## Why it's shaped this way
1. Persist the idempotency key before the network call — fix for IR-117.[^1]
2. Cap retries at 3 — fix for IR-091.[^2]
...

## Scars
> **IR-117 · 2024-09-04 · Double charge under partition**
> ...

## Tripwires
| ID     | Region              | Incident | Guarding test                                          |
|--------|---------------------|----------|--------------------------------------------------------|
| TW-001 | retry.py:27–28      | IR-117   | tests/.../test_idempotency_key_is_deterministic...     |

## Rejected alternatives
- **PR #2841 (closed)** — tenacity library. Closed because tenacity
  re-raises on permanent errors before the classifier runs, which
  would re-open IR-117.
```

---

## Architecture

```text
┌────────────────────────────────────────────────────────────────┐
│  IBM Bob (the IDE the developer is using)                      │
│  - Chat panel · Ask / Code / Plan / Review modes               │
│  - Agentic tool loop · multi-LLM routing (Granite + partners)  │
│  - Native trace / audit log                                    │
│                                                                │
│        Calls MCP tools ────────────────────┐                   │
└────────────────────────────────────────────┼───────────────────┘
                                             │
                                             ▼  stdio MCP transport
┌────────────────────────────────────────────────────────────────┐
│  Reckon MCP Server (`reckon-mcp`)                              │
│                                                                │
│  Internal modules (in `reckon` library):                       │
│    • Memoir Engine        — generates/updates MEMOIR.md        │
│    • Artifact Indexer     — git, GitHub (fixture), issues      │
│    • Postmortem Index     — embeddings + code_ref resolver     │
│    • Tripwire Checker     — diff vs tripwire registry          │
│    • Evidence Store       — SQLite, caches artifacts + traces  │
│    • Filesystem Writer    — writes MEMOIR.md into the repo     │
└────────────────────────────────────────────────────────────────┘
```

The CLI binary (`reckon`) and the MCP binary (`reckon-mcp`) share the
same library crate, so the warning block rendered by `reckon check` in
a standalone terminal and the warning Bob renders in Review mode are
produced by the same code path.

The MCP server speaks JSON-RPC over stdio directly (no rmcp SDK). The
SDK enforces a strict handshake that several real-world hosts skip;
hand-rolling the loop keeps Reckon compatible with Bob, Claude Desktop,
Cursor, and Continue without per-host workarounds.

---

## Tests

```sh
cargo test --tests        # 28 tests across 4 files
```

| File                        | What it covers                                                                                                               |
|-----------------------------|------------------------------------------------------------------------------------------------------------------------------|
| `tests/diff_parser.rs`      | Unified-diff parsing - empty input, multi-hunk, multi-file, ranges, sentinels, malformed headers, randomized property check. |
| `tests/tripwire_overlap.rs` | Exhaustive small-space test of `ranges_overlap`, plus path normalization.                                                    |
| `tests/postmortem_parse.rs` | Frontmatter extraction, BOM stripping, unclosed-block rejection.                                                             |
| `tests/end_to_end.rs`       | Tmpdir + real git history -> memoir generation -> tripwire check.                                                            |

---

## Non-goals

- Not a documentation generator. Memoirs are evidence-bound, not
  narrative summaries.
- Not a CVE scanner. Reckon prevents *historical* regressions, not
  generic vulnerabilities.
- Not a `git blame` replacement. Blame answers *who*. Reckon answers
  *why* — and *what was tried and rejected*.
- Not cross-repo. Single repo for the POC.

---

## One-line pitch

> reckon · for bob — gives every codebase a memory. It writes the
> history down, commits it next to the code, and stops you from
> forgetting it.
