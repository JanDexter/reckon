# Reckon slash commands for Bob

Suggested slash-command bindings for Bob's command palette. Each is a
thin wrapper over a single Reckon MCP tool.

| Slash command          | MCP tool                       | When to use                                                              |
|------------------------|--------------------------------|--------------------------------------------------------------------------|
| `/why <path>:<lines>`  | `reckon.explain`               | "Why is this code like this?" — grounded answer with footnoted sources.  |
| `/memoir <path>`       | `reckon.read_memoir`           | Render the MEMOIR.md for the module containing `<path>`.                 |
| `/memoir-regen <path>` | `reckon.update_memoir`         | Regenerate the memoir from the latest artifacts. Writes to disk.         |
| `/check`               | `reckon.check_diff`            | Run the staged diff through the tripwire registry. Hero command.         |
| `/postmortems <query>` | `reckon.search_postmortems`    | Semantic search over the postmortem corpus.                              |

Bob's agent should also call `reckon.check_diff` automatically when the
user enters Review mode (see `prompts/reckon-review-mode.md`) — the
slash command above is the manual escape hatch.
