# Installing Reckon

## 1. Build & install the binaries

From the project root:

```sh
cargo install --path .
```

This installs three binaries into `~/.cargo/bin/` (already on `$PATH`
for most Rust users):

```text
reckon         # CLI (`reckon why`, `reckon check`, `reckon memoir`, `reckon trace`)
reckon-mcp     # MCP server over stdio (the thing Bob calls)
reckon-seed    # demo-repo bootstrap
```

Verify:

```sh
reckon --version
reckon-mcp --help     # exits with a benign error if cwd is not a git repo
```

## 2. Seed the demo repo (optional, for the 90-second demo)

```sh
reckon-seed --out demo-repo --force
cd demo-repo
RECKON_REPO=$PWD reckon memoir payments/retry.py
RECKON_REPO=$PWD reckon check --diff demo-diff.patch
```

The third command should print the IR-117 tripwire warning block.

## 3. Wire Reckon into IBM Bob (or any MCP host)

MCP servers are configured the same way across every host. Bob, Claude
Desktop, Cursor, Continue, etc. all consume the same JSON shape under
their respective settings files. The contents of `bob/mcp.json` in this
repo are the canonical example:

```json
{
  "mcpServers": {
    "reckon": {
      "command": "reckon-mcp",
      "env": {
        "RECKON_REPO": "${workspaceFolder}"
      }
    }
  }
}
```

Where to put this depends on the host:

| Host                  | Config file (Windows)                                          |
|-----------------------|----------------------------------------------------------------|
| IBM Bob               | `%APPDATA%\Bob\mcp.json` (check Bob's settings UI for the path)|
| Claude Desktop        | `%APPDATA%\Claude\claude_desktop_config.json`                  |
| Cursor / Continue     | per-workspace `.cursor/mcp.json` or `~/.continuerc.json`       |

For IBM Bob specifically: open the settings panel, search for "MCP", and
either paste the `mcpServers.reckon` block into the JSON editor or point
Bob at `bob/mcp.json` from this repo. Restart Bob.

After Bob restarts it will spawn `reckon-mcp` over stdio with
`RECKON_REPO` set to the active workspace folder. The five Reckon tools
(`reckon.read_memoir`, `reckon.update_memoir`, `reckon.explain`,
`reckon.check_diff`, `reckon.search_postmortems`) become available in
Bob's chat and Review-mode flows.

## 4. Make Bob call Reckon automatically in Review mode

Drop the contents of `bob/prompts/reckon-review-mode.md` into Bob's
Review-mode system prompt template. The agent will then call
`reckon.check_diff` on every staged hunk and surface any tripwire match
**before** rendering its own review.

`bob/slash-commands.md` lists suggested slash-command bindings if you
prefer manual invocation.

## 5. Verify the wire

A smoke test of the MCP transport against the seeded demo repo:

```sh
cd demo-repo
RECKON_REPO=$PWD reckon-mcp <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
EOF
```

You should see one JSON-RPC response per request, the second of which
lists all five Reckon tools.

## Notes

- The first call to `reckon.search_postmortems` (or `reckon trace`)
  downloads a ~90 MB ONNX MiniLM-L6-v2 model under
  `%LOCALAPPDATA%/fastembed/`. Subsequent calls hit the cache.
- Every Reckon write is atomic: the engine writes to a `.reckon.tmp`
  sibling and renames over `MEMOIR.md` so a crash mid-write never leaves
  a corrupt memoir on disk.
- The `.reckon/` sidecar (SQLite evidence store + caches) is
  `.gitignore`d. `MEMOIR.md` files are intended to be committed.
