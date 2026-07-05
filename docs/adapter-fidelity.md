# Cross-surface token fidelity

Tokmeter separates two questions that often get conflated:

- How many total tokens or token-equivalent units were observed in the session?
- How many of those were attributable to git operations?

The answer can have different fidelity for each surface. Reports therefore use
explicit fidelity labels instead of treating every number as provider billing
data.

## Fidelity labels

- `exact`: token counts came from provider-reported usage fields or a structured
  usage export.
- `estimated`: token counts came from local byte/text estimation, hook payload
  size, MCP tool request/response size, or proxy payload size when no usage
  fields were exposed.
- `mixed`: at least one source was exact and at least one source was estimated.
- `unknown`: the event log did not include token-bearing records.

These labels describe token-count fidelity only. Passive sessions remain Grade
O evidence because the workload is observed rather than controlled. Use Mode T
when the study needs controlled baseline/treatment comparisons.

## Surface matrix

| Surface | Total session tokens | Git operation tokens | Recommended path |
| --- | --- | --- | --- |
| Codex CLI TUI with subscription auth | Estimated unless upstream usage is visible through the proxy | Estimated from hooks, proxy payloads, or tokmeter-managed git tools | `vc-tokmeter codex-tui` plus project hooks |
| Codex CLI with Platform API key | Exact when provider usage fields are present | Mixed when exact request totals are combined with estimated git hook/tool attribution | `vc-tokmeter codex-tui --provider api --keep-openai-api-key` or direct proxy |
| Codex `exec --json` | Exact for turns that emit structured usage | Estimated for local hook/MCP git attribution unless exact usage can be mapped to a git turn | Structured usage import plus hooks |
| Claude Desktop | Usually unavailable from the app itself | Estimated for git calls routed through tokmeter MCP tools | `vc-tokmeter mcp-git` |
| Claude Code CLI | Depends on whether the selected Anthropic/API path exposes usage | Estimated for hooks or proxy payloads when exact usage is unavailable | Claude wrapper/proxy path |
| Generic desktop app with MCP | Usually unavailable unless the app exports usage | Estimated for tokmeter MCP git tools | `vc-tokmeter mcp-git` |
| Generic transcript or telemetry import | Exact if usage fields are present, otherwise estimated | Depends on whether git metadata is present or can be joined to hook/MCP events | transcript or structured usage import |

## Reading the report

`report.md` contains a `Session git token share` section with:

- `Session total tokens`: all observed token-bearing records in the event log.
- `Git tokens`: records classified under git operation classes such as
  `vc.status`, `vc.diff`, `vc.log`, `vc.show`, `vc.branch_ops`, and
  `vc.push_pull`.
- `Non-git tokens`: session total minus git tokens.
- `Git token share`: git tokens divided by session total.
- `Token fidelity`: `exact`, `estimated`, `mixed`, or `unknown`.

`report.json` exposes the same fields under `session_git_share` for notebooks
and downstream study tooling.

Use `git_workflow` for action-level git rows. Use `token_sources` to understand
why the fidelity label was chosen, for example `proxy exact usage`,
`proxy estimated response`, `hook-estimated tool results`, `mcp tool request`,
or `mcp tool response`.

## Privacy boundary

Tokmeter event logs should not contain prompts, raw source, raw command output,
exact file paths, branch names, credentials, or provider secrets. Adapters store
counts, operation classes, normalized action labels, byte sizes, timestamps,
digests, and run metadata.

Desktop MCP tools return raw git output to the requesting app because the app
needs that data to work, but tokmeter persists only privacy-safe metadata about
that exchange.
