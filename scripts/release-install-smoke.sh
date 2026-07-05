#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
artifact=""
platform=""

usage() {
  cat <<EOF
Usage:
  scripts/release-install-smoke.sh --artifact PATH [--platform OS-ARCH]

Installs a packaged release tarball through install.sh, then exercises the
installed binary against a temporary target repository.
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 2
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --artifact)
      [ "$#" -ge 2 ] || die "--artifact requires a path"
      artifact="$2"
      shift 2
      ;;
    --platform)
      [ "$#" -ge 2 ] || die "--platform requires OS-ARCH"
      platform="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[ -n "$artifact" ] || die "--artifact is required"
[ -f "$artifact" ] || die "artifact does not exist: $artifact"

artifact="$(cd "$(dirname "$artifact")" && pwd)/$(basename "$artifact")"
artifact_name="$(basename "$artifact")"

if [ -z "$platform" ]; then
  case "$artifact_name" in
    vc-tokmeter-macos-arm64.tar.gz) platform="macos-arm64" ;;
    vc-tokmeter-macos-x64.tar.gz) platform="macos-x64" ;;
    vc-tokmeter-linux-arm64.tar.gz) platform="linux-arm64" ;;
    vc-tokmeter-linux-x64.tar.gz) platform="linux-x64" ;;
    *) die "cannot infer platform from artifact name: $artifact_name" ;;
  esac
fi

case "$platform" in
  macos-arm64) tokmeter_os="macos"; tokmeter_arch="arm64" ;;
  macos-x64) tokmeter_os="macos"; tokmeter_arch="x64" ;;
  linux-arm64) tokmeter_os="linux"; tokmeter_arch="arm64" ;;
  linux-x64) tokmeter_os="linux"; tokmeter_arch="x64" ;;
  *) die "unsupported platform: $platform" ;;
esac

work_root="${TMPDIR:-/tmp}/vc-tokmeter-release-smoke-$$"
release_dir="$work_root/release"
prefix="$work_root/install"
target_repo="$work_root/target-repo"
log_dir="$work_root/logs"

cleanup() {
  rm -rf "$work_root"
}
trap cleanup EXIT

mkdir -p "$release_dir" "$prefix" "$target_repo" "$log_dir"
cp "$artifact" "$release_dir/$artifact_name"
printf '%s  %s\n' "$(sha256_file "$release_dir/$artifact_name")" "$artifact_name" \
  >"$release_dir/SHA256SUMS"

TOKMETER_OS="$tokmeter_os" TOKMETER_ARCH="$tokmeter_arch" \
  "$repo_root/install.sh" \
  --base-url "$release_dir" \
  --prefix "$prefix" \
  >"$log_dir/install.log"

installed="$prefix/bin/vc-tokmeter"
test -x "$installed"
"$installed" --help >"$log_dir/help.log"
grep -q "vc-tokmeter" "$log_dir/help.log"

(
  cd "$target_repo"
  git init -q
  printf '# smoke target\n' >README.md
  git add README.md

  "$installed" doctor >"$log_dir/doctor-before.log"
  "$installed" setup --repo "$target_repo" --tokmeter-bin "$installed" \
    >"$log_dir/setup.log"

  test -f .codex/hooks.json
  grep -q "$installed" .codex/hooks.json
  printf 'keep-me\n' >.codex/foreign-config.txt

  mkdir -p .tokmeter
  : >.tokmeter/events.jsonl
  "$installed" report --event-log .tokmeter/events.jsonl --out .tokmeter/report \
    >"$log_dir/report.log"
  test -f .tokmeter/report/report.json
  test -f .tokmeter/report/report.md

  "$installed" uninstall >"$log_dir/uninstall.log"
  "$installed" doctor >"$log_dir/doctor-after.log"
)

test -f "$target_repo/.codex/foreign-config.txt"
if [ -f "$target_repo/.codex/hooks.json" ]; then
  if grep -q "vc-tokmeter" "$target_repo/.codex/hooks.json"; then
    printf 'tokmeter hook remained after uninstall\n' >&2
    exit 1
  fi
fi

grep -q "self-test" "$log_dir/doctor-after.log"
grep -q "checksum verified" "$log_dir/install.log"

printf 'result=pass\n'
printf 'artifact=%s\n' "$artifact_name"
printf 'installed=%s\n' "$installed"
