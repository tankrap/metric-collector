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

TOKMETER_OS=linux TOKMETER_ARCH=amd64 \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$prefix" >"$log_dir/install.log"

test -x "$prefix/bin/vc-tokmeter"
"$prefix/bin/vc-tokmeter" --help | grep -q "fake vc-tokmeter --help"
grep -q "checksum verified" "$log_dir/install.log"
grep -q "export PATH=\"$prefix/bin:" "$log_dir/install.log"
grep -q "did not edit your shell profiles" "$log_dir/install.log"
tar -tzf "$release_dir/$artifact" | grep -q "^vc-tokmeter$"

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
