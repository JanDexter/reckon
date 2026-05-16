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

- **`reckon-mcp`** — an MCP server exposing five tools over stdio.
- **`reckon`** — a CLI that shares the same `reckon` core library, so
  running `reckon check` inside Bob's terminal and running it standalone
  produce pixel-identical output.
- **`reckon-seed`** — a seeder that builds the synthetic demo
  repository (commits, PRs, issues, postmortems, tests) used to
  rehearse the 90-second demo.

> **Bob is the IDE and agent. Reckon is a capability Bob calls.**

---

## Build

```sh
cargo build --release
```

Produces three binaries under `target/release/`:

```text
target/release/reckon         # CLI
target/release/reckon-mcp     # MCP server (stdio)
target/release/reckon-seed    # demo-repo bootstrap
```

The first invocation of `reckon.search_postmortems` (or `reckon trace`)
downloads a ~90 MB ONNX MiniLM model under `~/.cache/fastembed/`.

---

## Demo (90 seconds)

```sh
# 1. seed the demo repo
cargo run --release --bin reckon-seed -- --out demo-repo --force

# 2. generate the memoir
RECKON_REPO=$PWD/demo-repo ./target/release/reckon memoir payments/retry.py

# 3. ask "why?"
RECKON_REPO=$PWD/demo-repo ./target/release/reckon why payments/retry.py:27-28

# 4. run the hero check against a staged diff that reverts IR-117
RECKON_REPO=$PWD/demo-repo ./target/release/reckon check --diff demo-diff.patch
```

Step 4 — `reckon check` — is the demo moment. The patch removes the
persisted idempotency key on lines 27–28. Reckon matches the region
against the `MEMOIR.md` tripwire for IR-117 and renders the warning
block.

Inside Bob, the same flow is invoked by the agent automatically when
the user enters Review mode (see `bob/prompts/reckon-review-mode.md`).

---

## MCP tools (the contract with Bob)

All tools return markdown when their output is meant to land in Bob's
chat. Structured tools return JSON.

| Tool                                | Returns         | Purpose                                                              |
|-------------------------------------|-----------------|----------------------------------------------------------------------|
| `reckon.read_memoir(path)`          | markdown        | Return the MEMOIR.md for the module containing `path`.               |
| `reckon.update_memoir(path)`        | markdown        | Regenerate the memoir from the latest artifacts. Writes to disk.     |
| `reckon.explain(repo,path,range)`   | markdown        | Grounded explanation of a code range. Memoir + blame + linked PRs.   |
| `reckon.check_diff(repo,path,diff)` | `{ warnings }`  | Match a unified diff against the tripwire registry. **Hero tool.**   |
| `reckon.search_postmortems(query)`  | `[ hits ]`      | Semantic search over the postmortem corpus.                          |

---

## Bob capability mapping

Reckon flows through Bob's native capabilities at every step.

| Feature                                            | Bob capability used                                |
|----------------------------------------------------|----------------------------------------------------|
| Domain tools (`explain`, `check_diff`, etc.)       | Bob's MCP tool calling                             |
| Synthesis of artifact bundles into rationale       | Bob's multi-LLM routing (Granite + partner models) |
| Trace / audit shown to the user                    | Bob's native audit log                             |
| Review-mode regression interception                | Bob's Review mode + MCP tool hooks                 |
| Conversational follow-up grounded in same context  | Bob's agent thread + MCP retrieval                 |
| Repo-wide context for memoir generation            | Bob's complete-repo context capability             |

---

## Wiring Reckon into Bob

Add the server to Bob's MCP configuration (`bob/mcp.json` is shipped as
a starting point):

```json
{
  "mcpServers": {
    "reckon": {
      "command": "reckon-mcp",
      "env": { "RECKON_REPO": "${workspaceFolder}" }
    }
  }
}
```

Drop `bob/prompts/reckon-review-mode.md` into Bob's Review-mode prompt
template. The agent will then call `reckon.check_diff` automatically on
every staged change. The slash commands in `bob/slash-commands.md` give
the developer a manual escape hatch.

---

## Memoir shape

A `MEMOIR.md` is committed to git next to the module it describes. The
frontmatter is the **machine-readable** layer: `tripwires` live here —
that is what `reckon.check_diff` matches against. The body is the
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
same library crate, so the warning block rendered by `reckon check` in a
standalone terminal and the warning Bob renders in Review mode are
produced by the same code path.

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
