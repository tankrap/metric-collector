# Desktop app git metrics with MCP

Desktop agents often do not expose exact per-turn token usage. Tokmeter can
still make git operation metrics consistent by routing git inspection through a
local MCP server. The server returns git output to the desktop app and logs
privacy-safe metrics to `.tokmeter/events.jsonl`.

This path is useful for Claude Desktop and other MCP-capable desktop apps.

## Start with the tradeoff

MCP git tools can measure:

- Estimated tokens for git tool requests and responses.
- Git action subtype, such as `git.status` and `git.diff`.
- Operation class, such as `vc.status` and `vc.diff`.
- Bytes and digests for git output.
- Full-session git token share for git interactions routed through the MCP
  tools.

MCP git tools cannot measure exact total conversation tokens unless the desktop
app also exposes usage data through logs, telemetry, export, or an API/proxy
path. Reports label this as Grade O observational data.

## Claude Desktop configuration

Build or reference the `vc-tokmeter` binary, then add an MCP server entry that
starts the git adapter from the repository you want to measure:

```json
{
  "mcpServers": {
    "tokmeter-git": {
      "command": "/Users/justin/metrics/target/debug/vc-tokmeter",
      "args": [
        "mcp-git",
        "--workdir",
        "/path/to/repository",
        "--event-log",
        "/path/to/repository/.tokmeter/events.jsonl"
      ]
    }
  }
}
```

After restarting the desktop app, ask it to use the tokmeter git tools for git
inspection. Example prompts:

```text
Use tokmeter_git_status to inspect this repo, then summarize the state.
```

```text
Use tokmeter_git_diff to review the current diff and identify risky changes.
```

Available tools:

- `tokmeter_git_status`
- `tokmeter_git_diff`
- `tokmeter_git_log`
- `tokmeter_git_show`
- `tokmeter_git_branch`

## Generate reports

Generate a report any time during or after the desktop session:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- report \
  --event-log /path/to/repository/.tokmeter/events.jsonl \
  --out /path/to/repository/.tokmeter/report
```

Then inspect the git share:

```sh
rg -n 'Total tokens|Git total tokens|Git token share|git\\.' \
  /path/to/repository/.tokmeter/report/report.md
```

The `Git workflow tokens` section answers how many observed token-equivalent
units were spent on git operations, with rows by action subtype and direction.

## Privacy behavior

The MCP server does not persist raw command text or raw git output. It persists:

- normalized tool names,
- operation classes,
- action subtypes,
- request/response direction,
- token estimates,
- byte counts,
- digests.

The desktop app still receives the git output as the tool result because that is
the purpose of the tool call.
