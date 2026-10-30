#!/usr/bin/env bash
# Packs the release archives of one platform from already built programs.
#
#   scripts/package.sh --version v0.2.0 --target x86_64-unknown-linux-gnu \
#                      --bin-dir target/x86_64-unknown-linux-gnu/release --out dist \
#                      [--gui-dir STAGE]
#
# Writes ref-<kind>-<version>-<target>.tar.gz (.zip for Windows targets) to --out, for
# kind in tui, web, all and, with --gui-dir, gui. STAGE is the result of
# `cmake --install` of ref-gui (see gui/cmake/Deploy.cmake); it holds no ref-server.
# Each archive holds one folder of the same name:
#
#   ref-tui-<version>-<target>/   ref, ref-server, README*.md, LICENSE, docs/
#   ref-web-<version>-<target>/   ref-web, ref-server, README*.md, LICENSE, docs/
#   ref-gui-<version>-<target>/   the GUI, ref-server, README*.md, LICENSE, docs/
#   ref-all-<version>-<target>/   ref, ref-web, the GUI, ref-server, README*.md, LICENSE, docs/
#
# The GUI is the content of STAGE (ref-gui, lib/, plugins/, qml/, qt.conf) and on Linux also
# ref-gui.desktop and ref.png. On macOS it is ref.app with ref-server inside, signed; the
# `all` folder links its ref-server to that one.
#
# stdout gets a Markdown table of the archive sizes; progress goes to stderr.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHARED_FILES=(README.md README.zh-CN.md LICENSE)
SHARED_DIRS=(docs)

KINDS=(tui web gui all)
GUI_APP_BUILT="ref-gui.app"
GUI_APP="ref.app"
LINUX_DESKTOP_FILE="$REPO_ROOT/scripts/package/ref-gui.desktop"
LINUX_ICON="$REPO_ROOT/assets/app/png/ref-256.png"

# Prints the interface programs of a package kind, from the build. ref-server is part of every
# package and not listed; the GUI comes from --gui-dir.
kind_binaries() {
  case "$1" in
    tui) echo "ref" ;;
    web) echo "ref-web" ;;
    gui) echo "" ;;
    all) echo "ref ref-web" ;;
  esac
}

kind_has_gui() {
  [[ -n "$GUI_DIR" && ( "$1" == gui || "$1" == all ) ]]
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

# Copies the programs, the server and the shared files into a new folder $stage/$name.
stage_package() {
  local kind=$1 name=$2 stage=$3
  local folder="$stage/$name" binary

  mkdir -p "$folder"
  for binary in $(kind_binaries "$kind"); do
    cp "$BIN_DIR/$binary$EXE_SUFFIX" "$folder/"
  done
  if kind_has_gui "$kind"; then
    stage_gui "$folder"
  else
    cp "$BIN_DIR/ref-server$EXE_SUFFIX" "$folder/"
  fi
  cp "${SHARED_FILES[@]/#/$REPO_ROOT/}" "$folder/"
  cp -R "${SHARED_DIRS[@]/#/$REPO_ROOT/}" "$folder/"
}

# Copies the GUI, and the ref-server next to it, into $1. The macOS app has its own
# ref-server inside, so the folder gets only a link to it.
stage_gui() {
  local folder=$1
  cp -R "$PREPARED_GUI_DIR/." "$folder/"
  if [[ -d "$folder/$GUI_APP" ]]; then
    ln -s "$GUI_APP/Contents/MacOS/ref-server" "$folder/ref-server"
  else
    cp "$BIN_DIR/ref-server$EXE_SUFFIX" "$folder/"
  fi
}

# Makes the GUI folder that every GUI package starts from: $PREPARED_GUI_DIR.
prepare_gui() {
  local stage=$1
  PREPARED_GUI_DIR="$stage/gui"
  cp -R "$GUI_DIR" "$PREPARED_GUI_DIR"

  if [[ -d "$PREPARED_GUI_DIR/$GUI_APP_BUILT" ]]; then
    mv "$PREPARED_GUI_DIR/$GUI_APP_BUILT" "$PREPARED_GUI_DIR/$GUI_APP"
    cp "$BIN_DIR/ref-server" "$PREPARED_GUI_DIR/$GUI_APP/Contents/MacOS/"
    "$REPO_ROOT/scripts/sign-macos-app.sh" "$PREPARED_GUI_DIR/$GUI_APP" >&2
  elif [[ "$TARGET" == *linux* ]]; then
    cp "$LINUX_DESKTOP_FILE" "$PREPARED_GUI_DIR/"
    cp "$LINUX_ICON" "$PREPARED_GUI_DIR/ref.png"
  fi
}

# Archives the folder $stage/$name as $OUT_DIR/$name.tar.gz or .zip.
archive_package() {
  local name=$1 stage=$2

  if [[ -n "$EXE_SUFFIX" ]]; then
    (cd "$stage" && 7z a -tzip "$OUT_DIR/$name.zip" "$name" > /dev/null)
  else
    # COPYFILE_DISABLE: macOS tar would add its own "._" files for extended attributes.
    COPYFILE_DISABLE=1 tar -C "$stage" -czf "$OUT_DIR/$name.tar.gz" "$name"
  fi
}

main() {
  local version=""
  TARGET="" BIN_DIR="" OUT_DIR="" GUI_DIR=""
  while (($#)); do
    case "$1" in
      --version) version=${2:?}; shift 2 ;;
      --target) TARGET=${2:?}; shift 2 ;;
      --bin-dir) BIN_DIR=${2:?}; shift 2 ;;
      --out) OUT_DIR=${2:?}; shift 2 ;;
      --gui-dir) GUI_DIR=${2:?}; shift 2 ;;
      *) usage ;;
    esac
  done
  [[ -n "$version" && -n "$TARGET" && -n "$BIN_DIR" && -n "$OUT_DIR" ]] || usage
  local target=$TARGET

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
  [[ -z "$GUI_DIR" ]] || GUI_DIR="$(cd "$GUI_DIR" && pwd)"
  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE"' EXIT
  [[ -z "$GUI_DIR" ]] || prepare_gui "$STAGE"

  echo "| Package | Size |"
  echo "|---|---|"
  local kind name
  for kind in "${KINDS[@]}"; do
    [[ "$kind" != gui || -n "$GUI_DIR" ]] || continue
    name="ref-$kind-$version-$target"
    log "packing $name"
    stage_package "$kind" "$name" "$STAGE"
    archive_package "$name" "$STAGE"
    echo "| \`$name$archive_suffix\` | $(size_kib "$OUT_DIR/$name$archive_suffix") |"
  done
}

main "$@"
