#!/usr/bin/env bash
# Packs the release archives of one platform from already built binaries.
#
#   scripts/package.sh --version v0.1.0 --target x86_64-unknown-linux-gnu \
#                      --bin-dir target/x86_64-unknown-linux-gnu/release --out dist
#
# Writes ref-<kind>-<version>-<target>.tar.gz (.zip for Windows targets) to --out, for
# kind in tui, web, all. Each archive holds one folder of the same name:
#
#   ref-tui-<version>-<target>/   ref, ref-server, README*.md, LICENSE, docs/
#   ref-web-<version>-<target>/   ref-web, ref-server, README*.md, LICENSE, docs/
#   ref-all-<version>-<target>/   ref, ref-web, ref-server, README*.md, LICENSE, docs/
#
# stdout gets a Markdown table of the archive sizes; progress goes to stderr.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHARED_FILES=(README.md README.zh-CN.md LICENSE)
SHARED_DIRS=(docs)

KINDS=(tui web all)

# Prints the executables of a package kind. ref-server is part of every package and not listed.
kind_binaries() {
  case "$1" in
    tui) echo "ref" ;;
    web) echo "ref-web" ;;
    all) echo "ref ref-web" ;;
  esac
}

usage() {
  echo "usage: $0 --version V --target TRIPLE --bin-dir DIR --out DIR" >&2
  exit 2
}

log() { echo "$*" >&2; }

die() {
  log "error: $*"
  exit 1
}

# Prints the size of a file in KiB with one decimal, e.g. "12345.6 KiB".
size_kib() {
  local bytes
  bytes=$(wc -c < "$1")
  awk -v b="$bytes" 'BEGIN { printf "%.1f KiB", b / 1024 }'
}

# Copies the executables, the server and the shared files into a new folder $stage/$name.
stage_package() {
  local kind=$1 name=$2 stage=$3
  local folder="$stage/$name" binary

  mkdir -p "$folder"
  for binary in $(kind_binaries "$kind") ref-server; do
    cp "$BIN_DIR/$binary$EXE_SUFFIX" "$folder/"
  done
  cp "${SHARED_FILES[@]/#/$REPO_ROOT/}" "$folder/"
  cp -R "${SHARED_DIRS[@]/#/$REPO_ROOT/}" "$folder/"
}

# Archives the folder $stage/$name as $OUT_DIR/$name.tar.gz or .zip.
archive_package() {
  local name=$1 stage=$2

  if [[ -n "$EXE_SUFFIX" ]]; then
    (cd "$stage" && 7z a -tzip "$OUT_DIR/$name.zip" "$name" > /dev/null)
  else
    tar -C "$stage" -czf "$OUT_DIR/$name.tar.gz" "$name"
  fi
}

main() {
  local version="" target=""
  BIN_DIR="" OUT_DIR=""
  while (($#)); do
    case "$1" in
      --version) version=${2:?}; shift 2 ;;
      --target) target=${2:?}; shift 2 ;;
      --bin-dir) BIN_DIR=${2:?}; shift 2 ;;
      --out) OUT_DIR=${2:?}; shift 2 ;;
      *) usage ;;
    esac
  done
  [[ -n "$version" && -n "$target" && -n "$BIN_DIR" && -n "$OUT_DIR" ]] || usage

  EXE_SUFFIX=""
  [[ "$target" == *windows* ]] && EXE_SUFFIX=".exe"
  local archive_suffix=".tar.gz"
  [[ -n "$EXE_SUFFIX" ]] && archive_suffix=".zip"

  local binary
  for binary in ref ref-web ref-server; do
    [[ -f "$BIN_DIR/$binary$EXE_SUFFIX" ]] || die "$BIN_DIR/$binary$EXE_SUFFIX not found"
  done

  mkdir -p "$OUT_DIR"
  OUT_DIR="$(cd "$OUT_DIR" && pwd)"
  BIN_DIR="$(cd "$BIN_DIR" && pwd)"
  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE"' EXIT

  echo "| Package | Size |"
  echo "|---|---|"
  local kind name
  for kind in "${KINDS[@]}"; do
    name="ref-$kind-$version-$target"
    log "packing $name"
    stage_package "$kind" "$name" "$STAGE"
    archive_package "$name" "$STAGE"
    echo "| \`$name$archive_suffix\` | $(size_kib "$OUT_DIR/$name$archive_suffix") |"
  done
}

main "$@"
