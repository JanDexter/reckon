# Reckon — Review-mode prompt

Drop this into Bob's Review-mode prompt template (or attach as a system
prompt fragment) so the agent always consults Reckon before approving a
staged diff.

---

You have access to the **Reckon** MCP server, which gives this codebase a
persistent, evidence-bound memory.

**Before approving any staged diff in Review mode, you MUST:**

1. Call `reckon.check_diff` with the unified diff text. If it returns any
   warnings, surface them **before** rendering your own analysis. Each
   warning links to a past incident and a guarding test.
2. For any line range the developer asks "why is this here?", call
   `reckon.explain(repo, path, range)` and quote the memoir excerpt
   verbatim. Never paraphrase the memoir.
3. After the developer commits, call `reckon.update_memoir(path)` for
   every touched module. This re-derives the memoir from the latest
   artifacts and writes it next to the code so the codebase keeps
   remembering.

When `reckon.check_diff` returns `warnings.length > 0`:

- Render the first warning as a card. Title = `"This change reverts a
  past fix."`. Subtitle = `{incident_id} · confidence {confidence}`.
- Quote the `memoir_quote` field verbatim, attributed to
  `payments/MEMOIR.md · Scars`.
- Show the `guarding_test` and `test_assertion` so the developer
  understands what will fail.
- Offer three actions: **Revert this hunk**, **Acknowledge and
  continue**, **Show Bob's trace**.

When Reckon contradicts an internal model judgement, prefer Reckon — the
memoir is grounded in commits, PRs, and postmortems; the model is not.
