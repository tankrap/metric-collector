#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_root="${TMPDIR:-/tmp}/vc-tokmeter-onboarding-smoke-$$"
checkout="$work_root/metrics"
log_dir="$work_root/logs"

cleanup() {
  rm -rf "$work_root"
}
trap cleanup EXIT

mkdir -p "$checkout" "$log_dir"

(
  cd "$repo_root"
  tar --exclude .git --exclude target --exclude .tokmeter -cf - .
) | (
  cd "$checkout"
  tar -xf -
)

start_seconds="$(date +%s)"

cd "$checkout"
cargo run -- init >"$log_dir/init.log"
cargo run -- status >"$log_dir/status.log"
cargo run -- report --out .tokmeter/report >"$log_dir/report.log"

test -f .tokmeter/report/report.json
test -f .tokmeter/report/report.md

first_report_seconds="$(( $(date +%s) - start_seconds ))"

cargo run -- doctor >"$log_dir/doctor.log"
cargo run -- uninstall >"$log_dir/uninstall.log"

printf 'first_report_seconds=%s\n' "$first_report_seconds"
printf 'threshold_seconds=300\n'

if [ "$first_report_seconds" -gt 300 ]; then
  printf 'result=fail\n'
  exit 1
fi

printf 'result=pass\n'
