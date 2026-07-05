#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_root="${TMPDIR:-/tmp}/vc-tokmeter-study-smoke-$$"
event_log="$work_root/.tokmeter/events.jsonl"
report_dir="$work_root/.tokmeter/report"

cleanup() {
  rm -rf "$work_root"
}
trap cleanup EXIT

mkdir -p "$(dirname "$event_log")" "$report_dir"

cat >"$event_log" <<'EOF'
schema=2	timestamp_ms=1700000000001	mode=passive	run_id=study-smoke	task_id=adhoc	profile_id=adhoc	adapter=proxy.ws.estimated	op_class=vc.status	tool=git-status	input_tokens=96	output_tokens=24	cache_read_tokens=0	cache_write_tokens=0	byte_count=480	digest=study-smoke-vc-status	repeat_of=	action_subtype=git.status	direction=response
schema=2	timestamp_ms=1700000000002	mode=passive	run_id=study-smoke	task_id=adhoc	profile_id=adhoc	adapter=proxy.ws.estimated	op_class=vc.diff	tool=git-diff	input_tokens=700	output_tokens=210	cache_read_tokens=140	cache_write_tokens=0	byte_count=4200	digest=study-smoke-vc-diff	repeat_of=	action_subtype=git.diff	direction=response
schema=2	timestamp_ms=1700000000003	mode=passive	run_id=study-smoke	task_id=adhoc	profile_id=adhoc	adapter=codex-hook	op_class=file.read	tool=read	input_tokens=0	output_tokens=300	cache_read_tokens=0	cache_write_tokens=0	byte_count=1200	digest=study-smoke-file-read	repeat_of=	action_subtype=	direction=response
EOF

cargo run --manifest-path "$repo_root/Cargo.toml" -- report \
  --event-log "$event_log" \
  --out "$report_dir" \
  >"$work_root/report.stdout"

test -f "$report_dir/report.json"
test -f "$report_dir/report.md"

rg '"total_tokens": 1470' "$report_dir/report.json" >/dev/null
rg '"token_sources":' "$report_dir/report.json" >/dev/null
rg '"source": "proxy estimate response"' "$report_dir/report.json" >/dev/null
rg '"operation_class": "vc.status"' "$report_dir/report.json" >/dev/null
rg '"tokens": 120' "$report_dir/report.json" >/dev/null
rg '"operation_class": "vc.diff"' "$report_dir/report.json" >/dev/null
rg '"tokens": 1050' "$report_dir/report.json" >/dev/null
rg '"direction": "response"' "$report_dir/report.json" >/dev/null
rg '## Token source breakdown' "$report_dir/report.md" >/dev/null
rg 'adapter=proxy.ws.estimated' "$event_log" >/dev/null

printf 'result=pass\n'
printf 'total_tokens=1470\n'
printf 'git_classes=vc.status,vc.diff\n'
