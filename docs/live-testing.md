# Live agent testing

Use `scripts/live-agent-matrix.sh` to exercise real agent surfaces without
hand-assembling commands. The helper does not create synthetic token events.
Each surface either launches a real agent path or prints the manual desktop
configuration needed for live testing.

Run commands from the `metric-collector` checkout and pass the repository you
want to measure:

```sh
scripts/live-agent-matrix.sh doctor --repo /path/to/repository
```

## Exact token paths

Use exact paths when the provider or CLI emits structured usage fields.

Codex non-interactive JSON:

```sh
scripts/live-agent-matrix.sh codex-exec --repo /path/to/repository
```

Codex interactive API-key proxy:

```sh
export OPENAI_API_KEY=...
scripts/live-agent-matrix.sh codex-tui-api --repo /path/to/repository
```

Claude Code API-key proxy:

```sh
export ANTHROPIC_API_KEY=...
scripts/live-agent-matrix.sh claude-code-api --repo /path/to/repository
```

## Live estimated paths

Subscription and desktop apps may not expose provider usage fields. These paths
still use real live sessions, but reports may label token fidelity as
`estimated` or `mixed`.

Codex subscription:

```sh
scripts/live-agent-matrix.sh codex-tui-subscription --repo /path/to/repository
```

Claude Code subscription:

```sh
scripts/live-agent-matrix.sh claude-code-subscription --repo /path/to/repository
```

Claude Desktop MCP config:

```sh
scripts/live-agent-matrix.sh claude-desktop-config --repo /path/to/repository
```

After adding the printed MCP config to Claude Desktop and restarting the app,
ask Claude Desktop to use `tokmeter_git_status` and `tokmeter_git_diff`.
Generate the report with:

```sh
scripts/live-agent-matrix.sh report --repo /path/to/repository
```

## Local MCP sanity check

This checks the MCP server and report wiring with real local git output. It does
not call a model provider:

```sh
scripts/live-agent-matrix.sh mcp-stdio-check --repo /path/to/repository
```

## Read the result

The helper prints the report path and lines matching the important study
fields:

- `Total tokens`
- `Session git token share`
- `Git tokens`
- `Git token share`
- `Token fidelity`
- token-source labels such as `codex exec exact usage`, `proxy exact usage`,
  `proxy estimate`, `mcp tool`, and `hook`

For the full report:

```sh
sed -n '1,160p' /path/to/repository/.tokmeter/report/report.md
```
