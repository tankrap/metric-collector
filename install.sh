#!/bin/sh
set -eu

repo="${TOKMETER_REPO:-tankrap/metric-collector}"
version="${TOKMETER_VERSION:-latest}"
prefix="${TOKMETER_PREFIX:-$HOME/.local}"
bin_dir="${TOKMETER_BIN_DIR:-}"
base_url="${TOKMETER_DOWNLOAD_BASE:-}"
dry_run=0
binary_name="vc-tokmeter"

usage() {
  cat <<'EOF'
Usage: install.sh [options]

Install vc-tokmeter from the latest GitHub release.

Options:
  --prefix DIR       Install under DIR/bin (default: $HOME/.local)
  --bin-dir DIR      Install directly into DIR instead of PREFIX/bin
  --version VERSION  GitHub release tag to install (default: latest)
  --repo OWNER/REPO  GitHub repository (default: tankrap/metric-collector)
  --base-url URL     Release artifact base URL or local directory
  --dry-run          Print planned install details without downloading
  -h, --help         Show this help

Environment:
  TOKMETER_PREFIX, TOKMETER_BIN_DIR, TOKMETER_VERSION, TOKMETER_REPO,
  TOKMETER_DOWNLOAD_BASE, TOKMETER_OS, and TOKMETER_ARCH mirror the options
  above. TOKMETER_OS and TOKMETER_ARCH are intended for installer tests.
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

log() {
  printf '%s\n' "$*"
}

need_arg() {
  [ "$#" -ge 2 ] || die "$1 requires a value"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      need_arg "$@"
      prefix="$2"
      shift 2
      ;;
    --bin-dir)
      need_arg "$@"
      bin_dir="$2"
      shift 2
      ;;
    --version)
      need_arg "$@"
      version="$2"
      shift 2
      ;;
    --repo)
      need_arg "$@"
      repo="$2"
      shift 2
      ;;
    --base-url)
      need_arg "$@"
      base_url="$2"
      shift 2
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

if [ -z "$bin_dir" ]; then
  bin_dir="$prefix/bin"
fi

detect_os() {
  raw_os="${TOKMETER_OS:-$(uname -s)}"
  case "$raw_os" in
    Darwin | darwin | macOS | macos) printf 'macos\n' ;;
    Linux | linux) printf 'linux\n' ;;
    *) die "unsupported operating system: $raw_os" ;;
  esac
}

detect_arch() {
  raw_arch="${TOKMETER_ARCH:-$(uname -m)}"
  case "$raw_arch" in
    arm64 | aarch64) printf 'arm64\n' ;;
    x86_64 | amd64 | x64) printf 'x64\n' ;;
    *) die "unsupported architecture: $raw_arch" ;;
  esac
}

os="$(detect_os)"
arch="$(detect_arch)"
artifact="vc-tokmeter-$os-$arch.tar.gz"

if [ -z "$base_url" ]; then
  if [ "$version" = "latest" ]; then
    base_url="https://github.com/$repo/releases/latest/download"
  else
    base_url="https://github.com/$repo/releases/download/$version"
  fi
fi
base_url="${base_url%/}"

archive_url="$base_url/$artifact"
checksums_url="$base_url/SHA256SUMS"

log "vc-tokmeter installer"
log "repo=$repo"
log "version=$version"
log "platform=$os/$arch"
log "artifact=$artifact"
log "install_dir=$bin_dir"

if [ "$dry_run" -eq 1 ]; then
  log "dry_run=true"
  log "would_download=$archive_url"
  log "would_verify=$checksums_url"
  log "would_install=$bin_dir/$binary_name"
  exit 0
fi

download() {
  src="$1"
  dst="$2"

  case "$src" in
    /*)
      cp "$src" "$dst"
      ;;
    file://*)
      if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$src" -o "$dst"
      else
        path="${src#file://}"
        cp "$path" "$dst"
      fi
      ;;
    http://* | https://*)
      if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$src" -o "$dst"
      elif command -v fetch >/dev/null 2>&1; then
        fetch -q -o "$dst" "$src"
      else
        die "curl or fetch is required to download release artifacts"
      fi
      ;;
    *)
      cp "$src" "$dst"
      ;;
  esac
}

sha256_file() {
  file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    die "sha256sum or shasum is required to verify checksums"
  fi
}

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vc-tokmeter-install.XXXXXX")"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

archive="$tmp_dir/$artifact"
checksums="$tmp_dir/SHA256SUMS"
extract_dir="$tmp_dir/extract"

log "downloading $archive_url"
download "$archive_url" "$archive"

log "downloading $checksums_url"
download "$checksums_url" "$checksums"

expected_hash="$(awk -v file="$artifact" '$2 == file || $2 == "*" file { print $1; exit }' "$checksums")"
[ -n "$expected_hash" ] || die "no checksum entry found for $artifact"

actual_hash="$(sha256_file "$archive")"
if [ "$actual_hash" != "$expected_hash" ]; then
  die "checksum mismatch for $artifact"
fi
log "checksum verified"

mkdir -p "$extract_dir"
tar -xzf "$archive" -C "$extract_dir"

binary_path="$(find "$extract_dir" -type f -name "$binary_name" | sed -n '1p')"
[ -n "$binary_path" ] || die "archive did not contain $binary_name"

mkdir -p "$bin_dir"
tmp_install="$bin_dir/.$binary_name.tmp.$$"
cp "$binary_path" "$tmp_install"
chmod 755 "$tmp_install"
mv "$tmp_install" "$bin_dir/$binary_name"

log "installed $bin_dir/$binary_name"

case ":$PATH:" in
  *:"$bin_dir":*)
    log "$bin_dir is already on PATH"
    ;;
  *)
    log ""
    log "Add vc-tokmeter to your PATH for this shell:"
    log "  export PATH=\"$bin_dir:\$PATH\""
    log ""
    log "The installer did not edit your shell profiles."
    ;;
esac

log "Try it:"
log "  vc-tokmeter --help"
