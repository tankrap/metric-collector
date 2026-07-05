# Install and App Flows

This guide explains which `vc-tokmeter` path to use for each app surface.

## One-line install

Install the latest GitHub release:

```sh
curl -fsSL https://raw.githubusercontent.com/tankrap/metric-collector/main/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
vc-tokmeter --help
```

The installer downloads the matching release tarball from GitHub Releases,
verifies it with `SHA256SUMS`, and installs into `$HOME/.local/bin` by default.

By default it installs:

- `vc-tokmeter`: the main CLI.
- `codex`: a tokmeter shim for Codex CLI/TUI sessions.
- `claude`: a tokmeter shim for Claude Code CLI sessions.

The installer does not edit shell profiles. If `$HOME/.local/bin` is not first
on `PATH`, add it in the shell where you run agent sessions:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## Configure a repository

Run setup from the repository you want to measure:

```sh
cd /path/to/repository
vc-tokmeter setup
vc-tokmeter doctor
```

To enroll that repo for hosted uploads, include the collector endpoint and
upload token:

```sh
vc-tokmeter setup \
  --upload-endpoint https://collector.example.test/v1/uploads \
  --upload-token "$VC_TOKMETER_UPLOAD_TOKEN"
```

Setup is local-only unless both upload flags are provided.

## Codex CLI or TUI

Use this path for normal terminal Codex work:

```sh
cd /path/to/repository
codex
```

The installed `codex` shim starts `vc-tokmeter codex-tui`, launches the real
Codex binary, and points that session at a localhost-only proxy. Each session
uses its own free localhost port, so multiple Codex sessions can run at the
same time.

Equivalent explicit command:

```sh
vc-tokmeter codex-tui
```

## Claude Code CLI

Use this path for terminal Claude Code:

```sh
cd /path/to/repository
claude
```

The installed `claude` shim starts `vc-tokmeter claude-code`, launches the real
Claude Code binary, and sets `ANTHROPIC_BASE_URL` for that child process. Each
session uses its own free localhost port.

Equivalent explicit command:

```sh
vc-tokmeter claude-code
```

GUI-launched apps usually do not inherit your shell `PATH`, so the `claude`
shim is for the CLI, not the Claude Desktop app.

## Claude Desktop GUI

Use MCP for Claude Desktop and other GUI apps that support MCP servers:

```sh
cd /path/to/repository
vc-tokmeter live-test claude-desktop-config --repo "$PWD"
```

Add the printed MCP server config to Claude Desktop, restart Claude Desktop,
then ask Claude to use the tokmeter git tools:

```text
Use tokmeter_git_status to inspect this repo, then summarize the state.
```

```text
Use tokmeter_git_diff to review the current diff and identify risky changes.
```

This measures git operations routed through the MCP tools. It does not
automatically capture every GUI conversation token unless the desktop app also
exposes usage data through another path.

## Reports and uploads

Generate a local report:

```sh
vc-tokmeter report --event-log .tokmeter/events.jsonl --out .tokmeter/report
sed -n '1,160p' .tokmeter/report/report.md
```

Review the redacted upload payload:

```sh
vc-tokmeter upload --dry-run
```

Send it only after review:

```sh
vc-tokmeter upload --yes
```

Upload sends a redacted aggregate payload. It does not upload source code,
prompts, raw tool output, branch names, repository URLs, request bodies, or
response text.

## Local collector test values

When testing against the local Docker collector from this repo:

```sh
vc-tokmeter setup \
  --upload-endpoint http://127.0.0.1:8088/v1/uploads \
  --upload-token test-upload
```

Dashboard:

```text
http://127.0.0.1:8088/dashboard
```

Dashboard admin token:

```text
test-admin
```
