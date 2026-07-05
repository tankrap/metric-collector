# External tester quickstart

This guide is for testers who only need to install `vc-tokmeter`, run one
10-15 minute AI-assisted git workflow, generate a local report, optionally
upload aggregate metrics, and uninstall.

vc-tokmeter is local-only by default. Setup, capture, report generation, and
uninstall do not upload data. Upload happens only when you explicitly run
`vc-tokmeter upload` with an endpoint and token.

## Install

Install the latest GitHub release:

```sh
curl -fsSL https://raw.githubusercontent.com/tankrap/metric-collector/main/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
vc-tokmeter --help
```

The default install also creates `codex` and `claude` command shims in the same
directory. When that directory is first on `PATH`, normal `codex` or `claude`
sessions run through tokmeter collection automatically.

For a fuller explanation of CLI shims versus GUI/MCP setup, see
[`install-and-app-flows.md`](install-and-app-flows.md).

## Pick a repository

Use a local repository where you are comfortable running an AI assistant. The
report stores aggregate counts and operation classes, not source code.

```sh
cd /path/to/repository
vc-tokmeter setup
vc-tokmeter doctor
```

`setup` installs local capture wiring for supported tools and prints every file
or config it changed. Upload stays disabled unless both an endpoint and token
are provided explicitly:

```sh
vc-tokmeter setup \
  --upload-endpoint https://collector.example.test/v1/uploads \
  --upload-token "$TOKMETER_UPLOAD_TOKEN"
```

## Run a 10-15 minute study session

Use one of these paths.

Codex CLI with installed helper commands:

```sh
vc-tokmeter live-test doctor --repo "$PWD"
vc-tokmeter live-test codex-exec --repo "$PWD" \
  --prompt "Inspect git status and the current diff, identify the main change, run one relevant validation command if it is obvious, and summarize risks without committing anything."
```

Codex interactive TUI:

```sh
codex
```

Claude Code:

```sh
claude
```

The explicit forms, `vc-tokmeter codex-tui` and `vc-tokmeter claude-code`, are
also supported. Each wrapped session uses its own free localhost proxy port, so
multiple Codex or Claude sessions can run at the same time.

Claude Desktop:

```sh
vc-tokmeter live-test claude-desktop-config --repo "$PWD"
```

Add the printed MCP config to Claude Desktop, restart Claude Desktop, then ask
it to use `tokmeter_git_status` and `tokmeter_git_diff`.

Copy-paste prompt for an interactive agent:

```text
Spend 10-15 minutes helping me understand this repository's current git state.
First inspect status and the current diff. Then identify the main files or
areas involved, summarize the intent of the changes, call out possible risks or
missing tests, and run one low-risk validation command if the right command is
obvious. Do not commit, push, or upload anything.
```

## Generate and read the report

```sh
vc-tokmeter report --event-log .tokmeter/events.jsonl --out .tokmeter/report
sed -n '1,160p' .tokmeter/report/report.md
```

Key fields:

- `Total tokens`: observed provider-reported or estimated token volume.
- `Session git token share`: git-related volume compared with the whole
  observed session.
- `Git workflow tokens`: git status, diff, log, show, branch, and push/pull
  rows.
- `Token fidelity`: `exact` when provider usage fields were observed,
  `estimated` when tokmeter estimated volume, `mixed` when both are present,
  and `unknown` when no token volume was available.

## Optional upload

Local reports are not uploaded automatically. Upload sends a redacted aggregate
payload based on the report-share boundary, not raw event logs.

Dry-run first:

```sh
vc-tokmeter upload \
  --out .tokmeter/report \
  --endpoint https://collector.example.test/upload \
  --token "$VC_TOKMETER_UPLOAD_TOKEN" \
  --dry-run
```

Send only after the study coordinator gives you the endpoint and token:

```sh
vc-tokmeter upload \
  --out .tokmeter/report \
  --endpoint https://collector.example.test/upload \
  --token "$VC_TOKMETER_UPLOAD_TOKEN" \
  --yes
```

## What is collected

Collected locally:

- operation classes such as `vc.status`, `vc.diff`, `file.read`, and
  `build.output`;
- timestamps, run labels, adapter labels, byte counts, token counts, cache
  token buckets, direction, and digests;
- generated `report.json`, `report.md`, and optional redacted upload payloads.

Not collected:

- source code, prompts, file contents, raw tool outputs, branch names,
  repository URLs, credential values, request bodies, or response text.

## Uninstall and cleanup

From the measured repository:

```sh
vc-tokmeter uninstall
vc-tokmeter doctor
```

Uninstall removes tokmeter-managed hook blocks and local setup files while
leaving unrelated config entries in place. It does not delete local evidence by
default. Remove reports and event logs manually when you no longer need them:

```sh
rm -rf .tokmeter
```
