# Installing Reckon

## 1. Build & install the binaries

From the project root:

```sh
cargo install --path .
```

This installs three binaries into `~/.cargo/bin/` (already on `$PATH`
for most Rust users):

```text
reckon         # CLI (init, doctor, install, memoir, why, check, trace, completions)
reckon-mcp     # MCP server over stdio (the thing Bob calls)
reckon-seed    # demo-repo bootstrap
```

Verify:

```sh
reckon --version
reckon doctor      # 5 preflight checks; runs from inside any git repo
```

## 2. Wire Reckon into your MCP host

`reckon install` atomically merges the `mcpServers.reckon` entry into
the host's MCP config without disturbing any other servers you have
configured:

```sh
reckon install bob        # -> ~/.bob/settings/mcp_settings.json
reckon install claude     # -> Claude Desktop platform-specific path
reckon install cursor     # -> ~/.cursor/mcp.json
reckon install print      # -> stdout (paste into any other host)
```

Flags: `--force` to overwrite an existing `reckon` entry, `--repo-path
PATH` to pin to a specific repo instead of `${workspaceFolder}`.

Then **restart the host**. Bob / Claude / Cursor spawn `reckon-mcp`
over stdio on startup, so a restart is required to pick up the new
server.

For reference, the JSON block `reckon install` writes is:

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

## 3. (Optional) Make Bob call Reckon automatically

Bob 1.x supports **Modes** — persona presets you can switch between.
Add a `Reckon Review` mode:

1. **Settings -> Modes -> +**.
2. **Slug**: `reckon`. **Name**: `Reckon Review`.
3. **Role definition**: paste the role section of
   `bob/prompts/reckon-review-mode.md`.
4. **Custom instructions**: paste the procedural rules from the same
   file (the "Reckon workflow rules" block).
5. **Available Tools**: check **Read files**, **Edit files**,
   **Execute commands**, **Use MCP**, **Switch modes**.
6. Save. Switch the chat to `Reckon Review` before reviewing a staged
   diff — Bob will call `reckon.check_diff` automatically.

## 4. Verify

```sh
cd /path/to/your/repo
reckon doctor
```

5-row preflight (git repo / `.reckon/` sidecar / `reckon-mcp` on PATH /
`MEMOIR.md` present / MCP host config detected) with a fix hint per
failed row.

## 5. Use it

```sh
reckon init --with-hooks         # scaffold .reckon/ + post-commit hook
reckon memoir src/               # generate the first MEMOIR.md
reckon check                     # match staged diff against tripwires
reckon why src/foo.rs:42-71      # grounded "why is this here?"
```

`reckon` auto-discovers the repo root by walking up from `$PWD` for
`.git/`, so you don't need `RECKON_REPO` once inside the repo. Pass
`--repo <dir>` to override.

For the 90-second demo against the synthetic IR-117 fixture:

```sh
reckon-seed --out demo-repo --force
cd demo-repo
reckon memoir payments/retry.py
reckon check --diff demo-diff.patch    # IR-117 tripwire fires
```

## Shell completions

```sh
reckon completions bash       > /etc/bash_completion.d/reckon
reckon completions zsh        > /usr/local/share/zsh/site-functions/_reckon
reckon completions fish       > ~/.config/fish/completions/reckon.fish
reckon completions powershell | Out-String | Invoke-Expression
```

## Notes

- First call to `reckon.search_postmortems` (or `reckon trace`)
  downloads a ~90 MB ONNX MiniLM-L6-v2 model under your platform cache
  dir (`~/.cache/fastembed/` on Linux, `%LOCALAPPDATA%\fastembed\` on
  Windows). Subsequent calls hit the cache.
- Every Reckon write is atomic: writes to a `.reckon.tmp` sibling, then
  renames over `MEMOIR.md`. A crash mid-write never leaves a corrupt
  memoir on disk.
- The `.reckon/` sidecar (SQLite evidence store + caches) is
  `.gitignore`d. `MEMOIR.md` files are intended to be committed.
- Smoke-test the MCP transport manually if `reckon doctor` is happy but
  the host still misbehaves:

  ```sh
  cd /path/to/repo
  reckon-mcp <<'EOF'
  {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.1"}}}
  {"jsonrpc":"2.0","method":"notifications/initialized"}
  {"jsonrpc":"2.0","id":2,"method":"tools/list"}
  EOF
  ```

  Should print two JSON-RPC responses, the second listing all five
  Reckon tools.
