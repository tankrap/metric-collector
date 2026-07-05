#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_root="${TMPDIR:-/tmp}/vc-tokmeter-install-smoke-$$"
release_dir="$work_root/release"
payload_dir="$work_root/payload"
prefix="$work_root/install"
bad_prefix="$work_root/install-bad"
log_dir="$work_root/logs"

cleanup() {
  rm -rf "$work_root"
}
trap cleanup EXIT

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

mkdir -p "$release_dir" "$payload_dir" "$log_dir"

artifact="vc-tokmeter-linux-x64.tar.gz"

cat >"$payload_dir/vc-tokmeter" <<'EOF'
#!/bin/sh
printf 'fake vc-tokmeter %s\n' "$*"
EOF
chmod +x "$payload_dir/vc-tokmeter"

tar -czf "$release_dir/$artifact" -C "$payload_dir" vc-tokmeter
hash="$(sha256_file "$release_dir/$artifact")"
printf '%s  %s\n' "$hash" "$artifact" >"$release_dir/SHA256SUMS"

TOKMETER_OS=linux TOKMETER_ARCH=amd64 \
  "$repo_root/install.sh" \
  --dry-run \
  --base-url "$release_dir" \
  --prefix "$prefix" >"$log_dir/dry-run.log"

grep -q "dry_run=true" "$log_dir/dry-run.log"
grep -q "would_install=$prefix/bin/vc-tokmeter" "$log_dir/dry-run.log"
grep -q "would_install=$prefix/bin/codex" "$log_dir/dry-run.log"
grep -q "would_install=$prefix/bin/claude" "$log_dir/dry-run.log"

TOKMETER_OS=linux TOKMETER_ARCH=amd64 \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$prefix" >"$log_dir/install.log"

test -x "$prefix/bin/vc-tokmeter"
test -x "$prefix/bin/codex"
test -x "$prefix/bin/claude"
"$prefix/bin/vc-tokmeter" --help | grep -q "fake vc-tokmeter --help"
CODEX_SANDBOX= CODEX_THREAD_ID= TOKMETER_CODEX_BIN=/usr/local/bin/codex-real \
  "$prefix/bin/codex" --version \
  | grep -q "fake vc-tokmeter codex-tui --codex-bin /usr/local/bin/codex-real -- --version"
TOKMETER_CLAUDE_BIN=/usr/local/bin/claude-real "$prefix/bin/claude" --version \
  | grep -q "fake vc-tokmeter claude-code --claude-bin /usr/local/bin/claude-real -- --version"
grep -q "checksum verified" "$log_dir/install.log"
grep -q "export PATH=\"$prefix/bin:" "$log_dir/install.log"
grep -q "did not edit your shell profiles" "$log_dir/install.log"
grep -q "installed $prefix/bin/codex" "$log_dir/install.log"
grep -q "installed $prefix/bin/claude" "$log_dir/install.log"
tar -tzf "$release_dir/$artifact" | grep -q "^vc-tokmeter$"

foreign_prefix="$work_root/install-foreign"
mkdir -p "$foreign_prefix/bin"
cat >"$foreign_prefix/bin/codex" <<'EOF'
#!/bin/sh
printf 'foreign codex\n'
EOF
chmod +x "$foreign_prefix/bin/codex"

TOKMETER_OS=linux TOKMETER_ARCH=amd64 \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$foreign_prefix" >"$log_dir/foreign-install.log"

"$foreign_prefix/bin/codex" | grep -q "foreign codex"
grep -q "skipped $foreign_prefix/bin/codex" "$log_dir/foreign-install.log"
test -x "$foreign_prefix/bin/claude"

force_prefix="$work_root/install-force"
real_agent_dir="$work_root/real-agents"
mkdir -p "$force_prefix/bin" "$real_agent_dir"
cat >"$real_agent_dir/codex" <<'EOF'
#!/bin/sh
printf 'real codex\n'
EOF
chmod +x "$real_agent_dir/codex"
ln -s "$real_agent_dir/codex" "$force_prefix/bin/codex"

TOKMETER_OS=linux TOKMETER_ARCH=amd64 \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$force_prefix" \
  --force-agent-aliases >"$log_dir/force-install.log"

CODEX_SANDBOX= CODEX_THREAD_ID= "$force_prefix/bin/codex" --version \
  | grep -q "fake vc-tokmeter codex-tui --codex-bin $real_agent_dir/codex -- --version"
grep -q "installed $force_prefix/bin/codex" "$log_dir/force-install.log"

TOKMETER_OS=linux TOKMETER_ARCH=amd64 \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$force_prefix" \
  --force-agent-aliases >"$log_dir/force-reinstall.log"

CODEX_SANDBOX= CODEX_THREAD_ID= "$force_prefix/bin/codex" --version \
  | grep -q "fake vc-tokmeter codex-tui --codex-bin $real_agent_dir/codex -- --version"

explicit_prefix="$work_root/install-explicit"
TOKMETER_OS=linux TOKMETER_ARCH=amd64 TOKMETER_CODEX_BIN="$real_agent_dir/codex" \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$explicit_prefix" >"$log_dir/explicit-install.log"

CODEX_SANDBOX= CODEX_THREAD_ID= "$explicit_prefix/bin/codex" --version \
  | grep -q "fake vc-tokmeter codex-tui --codex-bin $real_agent_dir/codex -- --version"
CODEX_SANDBOX=seatbelt "$explicit_prefix/bin/codex" --version \
  | grep -q "real codex"

printf '0000000000000000000000000000000000000000000000000000000000000000  %s\n' \
  "$artifact" >"$release_dir/SHA256SUMS"

if TOKMETER_OS=linux TOKMETER_ARCH=amd64 \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$bad_prefix" >"$log_dir/bad-checksum.log" 2>&1; then
  printf 'expected checksum failure\n' >&2
  exit 1
fi

grep -q "checksum mismatch" "$log_dir/bad-checksum.log"
test ! -e "$bad_prefix/bin/vc-tokmeter"

printf 'result=pass\n'
