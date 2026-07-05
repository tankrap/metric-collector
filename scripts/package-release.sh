#!/usr/bin/env bash
set -euo pipefail

target=""
package_name=""
binary_name="vc-tokmeter"
dist_dir="dist"

usage() {
  cat <<EOF
Usage:
  scripts/package-release.sh --target TARGET [--package-name NAME] [--dist DIR]

Packages target/TARGET/release/vc-tokmeter into versioned and stable tar.gz
archives, then writes matching SHA256 checksum files.
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 2
}

sha256_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file"
  else
    die "missing sha256sum or shasum"
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target)
      [ "$#" -ge 2 ] || die "--target requires a value"
      target="$2"
      shift 2
      ;;
    --package-name)
      [ "$#" -ge 2 ] || die "--package-name requires a value"
      package_name="$2"
      shift 2
      ;;
    --dist)
      [ "$#" -ge 2 ] || die "--dist requires a value"
      dist_dir="$2"
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

[ -n "$target" ] || die "--target is required"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/Cargo.toml"
binary="$repo_root/target/$target/release/$binary_name"

[ -f "$manifest" ] || die "missing Cargo.toml at $manifest"
[ -x "$binary" ] || die "missing executable release binary: $binary"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$manifest" | head -n 1)"
[ -n "$version" ] || die "could not read package version from Cargo.toml"

if [ -z "$package_name" ]; then
  package_name="$target"
fi

case "$dist_dir" in
  /*) dist_path="$dist_dir" ;;
  *) dist_path="$repo_root/$dist_dir" ;;
esac

archive_base="$binary_name-v$version-$package_name"
install_archive="$binary_name-$package_name.tar.gz"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/vc-tokmeter-package.XXXXXX")"
cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT

mkdir -p "$dist_path" "$work_dir/$archive_base"
cp "$binary" "$work_dir/$archive_base/$binary_name"
chmod 0755 "$work_dir/$archive_base/$binary_name"

cat >"$work_dir/$archive_base/README.txt" <<EOF
vc-tokmeter $version

Install:
  chmod +x $binary_name
  ./$binary_name --help
EOF

(
  cd "$work_dir"
  tar -czf "$dist_path/$archive_base.tar.gz" "$archive_base"
)

cp "$dist_path/$archive_base.tar.gz" "$dist_path/$install_archive"

(
  cd "$dist_path"
  sha256_file "$archive_base.tar.gz" >"$archive_base.tar.gz.sha256"
  sha256_file "$install_archive" >"$install_archive.sha256"
  {
    sha256_file "$archive_base.tar.gz"
    sha256_file "$install_archive"
  } > SHA256SUMS
)

printf 'archive=%s\n' "$dist_path/$archive_base.tar.gz"
printf 'install_archive=%s\n' "$dist_path/$install_archive"
printf 'checksum=%s\n' "$dist_path/$archive_base.tar.gz.sha256"
