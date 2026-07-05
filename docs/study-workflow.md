# Passive AI-assisted git token study

This workflow measures token-equivalent volume during normal AI-assisted git
work. It is intended for studies of how much context moves through current git
workflows when an AI agent is involved. It is not a billing report.

Passive reports are Grade O evidence: the work is observed as it happens, but
the workload is not controlled. Use the numbers to describe workflow token
volume, operation mix, and rough before/after patterns. Do not frame them as
savings claims unless the same work is later repeated under Mode T.

## Capture a session

Run these commands from the repository being measured. Replace
`/Users/justin/metrics` with the local `metric-collector` checkout if needed.

First install project-local hooks:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- init
```

The hooks append privacy-safe metadata to `.tokmeter/events.jsonl` for tool
events such as file reads, edits, test output, and git commands. They store
counts, operation classes, byte counts, and digests, not prompts, source code,
or raw tool output.

For richer token measurement with a long interactive Codex TUI session, launch
Codex through the tokmeter wrapper:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- codex-tui
```

This starts the local proxy, launches the normal interactive `codex` TUI in the
same terminal, points Codex at the proxy, continuously appends events to
`.tokmeter/events.jsonl`, and writes a report when Codex exits. While Codex is
still running, you can generate an interim report from another terminal:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

Pass Codex TUI arguments after `--`:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- codex-tui -- \
  --model gpt-5.5
```

For Codex with a Platform API key instead of subscription auth:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- codex-tui \
  --provider api \
  --keep-openai-api-key
```

For long interactive Claude Code sessions:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- claude-code
```

This starts the Anthropic proxy path, launches the normal `claude` CLI, appends
events to `.tokmeter/events.jsonl`, and writes a report after Claude exits. See
[claude-code.md](claude-code.md) for API-key and subscription mode caveats.

If you prefer to manage terminals yourself, you can still run the proxy
directly in one terminal and Codex in another:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- proxy \
  --bind-host 127.0.0.1 \
  --port 17683 \
  --upstream https://chatgpt.com/backend-api \
  --event-log .tokmeter/events.jsonl
```

```sh
unset OPENAI_API_KEY
codex -c chatgpt_base_url='"http://127.0.0.1:17683/backend-api/"'
```

Now work normally: ask the agent to inspect `git status`, review diffs, edit
files, run tests, and prepare commits. Stop the proxy after the session ends.

## Generate the report

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

Read the human summary:

```sh
sed -n '1,120p' .tokmeter/report/report.md
```

The summary table reports total observed token volume:

- `Total tokens`: all observed provider-reported or estimated tokens in the
  event log.
- `Input tokens`, `Output tokens`, `Cache read tokens`, and
  `Cache write tokens`: directional buckets when the adapter can distinguish
  them.
- `Bytes`: captured hook/proxy byte volume, useful when the provider path does
  not expose exact token fields.

When the subscription proxy cannot see provider usage fields, it records
estimated token-equivalent volume. Treat this as a consistent study scale for
workflow context, not as a provider invoice.

## Check proxy and git-specific records

Check that proxy token events were captured:

```sh
rg "adapter=proxy.ws|adapter=proxy.ws.estimated|adapter=proxy.estimated" \
  .tokmeter/events.jsonl
```

Check for git-related event classes:

```sh
rg "op_class=vc\\.(status|diff|log|show|branch_ops|push_pull)" \
  .tokmeter/events.jsonl
```

The markdown report includes a `Git workflow tokens` section with git totals
and per-action rows. Rows include action subtype, direction, operation class,
events, token buckets, bytes, and share of all observed tokens. The same
operation-class totals are also available in `report.json` under
`git_workflow`, `token_sources`, and `class_shares`:

```sh
rg -n 'Git workflow tokens|Git total tokens|git\\.' .tokmeter/report/report.md
rg -n '"git_workflow"|"token_sources"|"direction":|"operation_class": "vc\\.|"action_subtype": "git\\.' \
  .tokmeter/report/report.json
```

Use these classes to separate tokens for and from git actions:

- `vc.status`: status and porcelain checks.
- `vc.diff`: diff review and patch inspection.
- `vc.log`: history inspection.
- `vc.show`: commit or object inspection.
- `vc.branch_ops`: branch listing or branch changes.
- `vc.push_pull`: network sync operations.

Use `git_workflow` when you want study-specific git totals. Use `class_shares`
when you want to compare git volume with non-git classes such as file reads,
edit echoes, test output, and build output. Use `token_sources` when you want
to separate hook records, exact proxy usage, estimated proxy records, transcript
imports, and request/response direction where the adapter provides it.

Reports also include `Session git token share`, which summarizes total session
tokens, git tokens, non-git tokens, git share, and whether the token counts are
exact, estimated, mixed, or unknown. See
[adapter-fidelity.md](adapter-fidelity.md) for the support matrix across Codex,
Claude, desktop apps, MCP tools, and structured imports.

For Claude Desktop and other MCP-capable desktop apps, use the local MCP git
adapter so git operations flow through tokmeter-managed tools. See
[desktop-mcp.md](desktop-mcp.md).

## Local deterministic smoke test

Use the synthetic smoke test when changing docs, fixtures, or report wiring and
you do not want to run live Codex:

```sh
scripts/study-workflow-smoke.sh
```

The smoke test writes a private-content-free event log with non-zero proxy
estimated tokens and at least two git operation classes, generates a report,
and verifies that the report preserves total tokens, token source rows,
directional git rows, plus `vc.status` and `vc.diff` class totals.
