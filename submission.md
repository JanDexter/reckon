# Reckon — Hackathon Submission

---

## Basic Information

### Project Title

**Reckon**

### Short Description

*(255 characters max)*

> Reckon gives every codebase a memory. It auto-generates a MEMOIR.md per module from commits, PRs, and postmortems — then blocks code changes that would re-open past incidents, right inside IBM Bob.

*(197 characters)*

---

### Long Description

*(100+ words)*

Every engineering team has an IR-117. A painful incident, carefully fixed, with tests added — and then silently re-opened six months later when someone refactored two lines without knowing what they guarded. `git blame` tells you *who* changed the code. It can't tell you *why* it exists, what incident it prevents, or what alternatives were already tried and rejected. That knowledge lives in people's heads, and it leaves when they do.

Reckon fixes this by giving every module a persistent, version-controlled memory: a `MEMOIR.md` committed alongside the code. Each memoir is auto-generated from the repo's own artifacts — git history, pull requests, linked issues, and postmortem documents — and includes machine-readable **tripwires** that guard specific code regions tied to past incidents. When a developer stages a diff that touches a guarded region, `reckon check` fires a warning before the code ships.

Reckon ships as a Rust CLI and an MCP server that integrates natively with IBM Bob. In Bob's **Reckon Review Mode**, every staged hunk is automatically cross-checked against the tripwire registry. The `reckon why` command answers "why is this code like this?" with a full decision trail — the incident that caused it, the PRs that fixed it, and the alternatives that were rejected. The evidence store uses local SQLite and ONNX embeddings, so semantic search over postmortems is fast, private, and free.

**Target audience:** Engineering teams of any size that use git and care about incident prevention. Any codebase with a history of painful bugs is a candidate. The integration with IBM Bob makes Reckon a natural fit for enterprise teams already on that platform.

**Unique benefits:** Reckon is the only tool that ties code regions to incident postmortems and actively blocks their regression. It doesn't generate generic documentation — it generates evidence-bound memory, grounded in the repo's own history, committed next to the code it describes.

---

### Technology & Category Tags

**Primary technologies:**
`Rust` · `MCP (Model Context Protocol)` · `IBM Bob` · `SQLite` · `ONNX / MiniLM-L6-v2` · `Git`

**Category tags:**
`Developer Tools` · `AI / LLM Integration` · `Code Quality` · `Incident Prevention` · `Knowledge Management` · `CLI` · `IDE Plugin` · `Open Source`

---

## Demo Access for Judges

### Option A — GitHub Releases *(recommended)*

Download the pre-built binaries from the GitHub Releases page:

```
reckon      (CLI)
reckon-mcp  (MCP server)
reckon-seed (demo bootstrapper)
```

Then run the 90-second demo against the synthetic IR-117 fixture:

```sh
reckon-seed --out demo-repo --force
cd demo-repo
reckon memoir payments/retry.py
reckon why    payments/retry.py:27-28
reckon check --diff demo-diff.patch
```

The third command — `reckon check` — is the hero. It fires the IR-117 tripwire.

### Option B — Build from source

```sh
git clone <repo-url>
cd reckon
cargo install --path .
reckon doctor
```

### Wiring into IBM Bob

```sh
reckon install bob   # writes MCP config
# restart Bob
# switch to Reckon Review mode
```

---

## 4-Minute Demo Script

| Time      | What to show                                                                 |
|-----------|------------------------------------------------------------------------------|
| 0:00–0:30 | **The problem.** Tell the IR-117 story. 312 customers double-charged. Show the two lines that caused it. |
| 0:30–1:00 | **reckon-seed + reckon memoir.** Bootstrap the demo repo. Generate the MEMOIR.md. Show the tripwire block. |
| 1:00–1:45 | **reckon why.** Ask why `retry.py:27-28` exists. Show the full decision trail — IR-117 ref, rejected PR #2841, guarding test. |
| 1:45–2:30 | **reckon check.** Apply the bad patch. Watch the tripwire fire with the incident link and test reference. |
| 2:30–3:15 | **Bob Review Mode.** Show the same check happening automatically inside IBM Bob as a staged diff is reviewed. |
| 3:15–3:45 | **Market + Revenue.** TAM $29B+, SAM ~$3B. Open-core → SaaS → Enterprise path. |
| 3:45–4:00 | **CTA.** "Stop forgetting. Start reckoning." GitHub link, `reckon install bob`. |

---

## Hosting Recommendation

**Use GitHub Releases** for judge-accessible binaries. This is the right choice because:

- Reckon ships as static Rust binaries — no runtime dependencies, no Docker, no cloud account needed
- `reckon-seed` bootstraps a complete synthetic demo repo in one command, so judges can run the exact demo flow locally without cloning the main repo
- GitHub Releases gives direct download links you can embed in the submission, the README, and the slide deck
- Pre-build for the three major platforms: `x86_64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`

**Do not** use a hosted web demo — Reckon's value is in the terminal and the IDE. A web demo would misrepresent the product.

Add a `DEMO.md` or `JUDGING.md` to the repo root with the exact three-command demo flow so judges can run it in under 2 minutes.
