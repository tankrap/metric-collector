#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_repo="$PWD"
prompt="Inspect git status and the current diff, then summarize the repository state and any risks."
surface=""

usage() {
  cat <<EOF
Usage:
  scripts/live-agent-matrix.sh <surface> [--repo PATH] [--prompt TEXT]

Surfaces:
  doctor                    Check local prerequisites and configured paths
  codex-exec                Run real non-interactive Codex JSON, import usage, report
  codex-tui-api             Launch interactive Codex through API proxy
  codex-tui-subscription    Launch interactive Codex through subscription proxy
  claude-code-api           Launch interactive Claude Code through Anthropic API proxy
  claude-code-subscription  Launch interactive Claude Code through subscription proxy
  claude-desktop-config     Print Claude Desktop MCP config for the target repo
  mcp-stdio-check           Exercise tokmeter MCP git tools locally
  report                    Generate and summarize the current tokmeter report

Options:
  --repo PATH               Repository to measure. Defaults to current directory.
  --prompt TEXT             Prompt for codex-exec.

Examples:
  scripts/live-agent-matrix.sh doctor --repo /path/to/repo
  scripts/live-agent-matrix.sh codex-exec --repo /path/to/repo
  scripts/live-agent-matrix.sh codex-tui-api --repo /path/to/repo
  scripts/live-agent-matrix.sh claude-desktop-config --repo /path/to/repo
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 2
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

if [ "${1:-}" = "" ] || [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

surface="$1"
shift

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo)
      [ "$#" -ge 2 ] || die "--repo requires a path"
      target_repo="$2"
      shift 2
      ;;
    --prompt)
      [ "$#" -ge 2 ] || die "--prompt requires text"
      prompt="$2"
      shift 2
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[ -d "$target_repo" ] || die "repo does not exist: $target_repo"
target_repo="$(cd "$target_repo" && pwd)"
event_log="$target_repo/.tokmeter/events.jsonl"
report_dir="$target_repo/.tokmeter/report"
codex_jsonl="$target_repo/.tokmeter/codex-exec.jsonl"

tokmeter() {
  cargo run --manifest-path "$repo_root/Cargo.toml" -- "$@"
}

ensure_tokmeter_dir() {
  mkdir -p "$target_repo/.tokmeter"
}

init_capture() {
  (
    cd "$target_repo"
    tokmeter init >/dev/null
  )
}

write_report() {
  tokmeter report \
    --event-log "$event_log" \
    --out "$report_dir" \
    >/dev/null
}

print_report_summary() {
  printf 'event_log=%s\n' "$event_log"
  printf 'report_md=%s/report.md\n' "$report_dir"
  if [ -f "$report_dir/report.md" ]; then
    rg -n 'Total tokens|Session git token share|Git tokens|Git token share|Token fidelity|codex exec exact usage|proxy exact usage|proxy estimate|mcp tool|hook' \
      "$report_dir/report.md" || true
  fi
}

case "$surface" in
  doctor)
    printf 'tokmeter_repo=%s\n' "$repo_root"
    printf 'target_repo=%s\n' "$target_repo"
    printf 'event_log=%s\n' "$event_log"
    printf 'report_dir=%s\n' "$report_dir"
    require_command cargo
    require_command rg
    if command -v codex >/dev/null 2>&1; then
      printf 'codex=found:%s\n' "$(command -v codex)"
    else
      printf 'codex=missing\n'
    fi
    if command -v claude >/dev/null 2>&1; then
      printf 'claude=found:%s\n' "$(command -v claude)"
    else
      printf 'claude=missing\n'
    fi
    if [ -n "${OPENAI_API_KEY:-}" ]; then
      printf 'OPENAI_API_KEY=set\n'
    else
      printf 'OPENAI_API_KEY=unset\n'
    fi
    if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
      printf 'ANTHROPIC_API_KEY=set\n'
    else
      printf 'ANTHROPIC_API_KEY=unset\n'
    fi
    ;;

  codex-exec)
    require_command codex
    ensure_tokmeter_dir
    init_capture
    (
      cd "$target_repo"
      codex exec --json "$prompt" >"$codex_jsonl"
    )
    tokmeter import-usage \
      --source "$codex_jsonl" \
      --event-log "$event_log"
    write_report
    print_report_summary
    ;;

  codex-tui-api)
    [ -n "${OPENAI_API_KEY:-}" ] || die "OPENAI_API_KEY must be set for codex-tui-api"
    init_capture
    (
      cd "$target_repo"
      tokmeter codex-tui --provider api --keep-openai-api-key
    )
    write_report
    print_report_summary
    ;;

  codex-tui-subscription)
    init_capture
    (
      cd "$target_repo"
      tokmeter codex-tui
    )
    write_report
    print_report_summary
    ;;

  claude-code-api)
    [ -n "${ANTHROPIC_API_KEY:-}" ] || die "ANTHROPIC_API_KEY must be set for claude-code-api"
    init_capture
    (
      cd "$target_repo"
      tokmeter claude-code --keep-anthropic-api-key
    )
    write_report
    print_report_summary
    ;;

  claude-code-subscription)
    init_capture
    (
      cd "$target_repo"
      tokmeter claude-code
    )
    write_report
    print_report_summary
    ;;

  claude-desktop-config)
    ensure_tokmeter_dir
    cargo build --manifest-path "$repo_root/Cargo.toml" >/dev/null
    cat <<EOF
{
  "mcpServers": {
    "tokmeter-git": {
      "command": "$repo_root/target/debug/vc-tokmeter",
      "args": [
        "mcp-git",
        "--workdir",
        "$target_repo",
        "--event-log",
        "$event_log"
      ]
    }
  }
}
EOF
    printf '\nAfter adding this to Claude Desktop, ask it to use tokmeter_git_status and tokmeter_git_diff.\n'
    ;;

  mcp-stdio-check)
    ensure_tokmeter_dir
    printf '%s\n' \
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
      '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"tokmeter_git_status","arguments":{}}}' \
      | tokmeter mcp-git --workdir "$target_repo" --event-log "$event_log" >/tmp/vc-tokmeter-mcp-stdio-check.jsonl
    write_report
    print_report_summary
    printf 'mcp_stdout=/tmp/vc-tokmeter-mcp-stdio-check.jsonl\n'
    ;;

  report)
    write_report
    print_report_summary
    ;;

  *)
    usage
    die "unknown surface: $surface"
    ;;
esac
