# SPDX-License-Identifier: GPL-3.0-or-later
# Shared by install-qt.sh and build-deps.sh (sourced).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=versions.sh
source "$SCRIPT_DIR/versions.sh"

# Everything the scripts produce lives under one directory:
#   $REF_DEPS_DIR/Qt        Qt installed by aqtinstall
#   $REF_DEPS_DIR/prefix    ECM, KSyntaxHighlighting and QCoro
#   $REF_DEPS_DIR/aqt-venv  Python venv with aqtinstall
#   $REF_DEPS_DIR/work      downloads and build trees
DEPS_DIR="${REF_DEPS_DIR:-$SCRIPT_DIR/../.deps}"

die() {
	echo "error: $*" >&2
	exit 1
}

# Prints "<aqt host> <aqt arch>" for the machine running the script. Pass AQT_ARCH to
# override the arch (e.g. win64_msvc2022_arm64).
detect_aqt_target() {
	local os machine host arch
	os="$(uname -s)"
	machine="$(uname -m)"
	case "$os" in
	Linux)
		host=linux
		case "$machine" in
		x86_64) arch=linux_gcc_64 ;;
		aarch64) arch=linux_gcc_arm64 ;;
		*) die "unsupported CPU: $machine" ;;
		esac
		;;
	Darwin)
		host=mac
		arch=clang_64
		;;
	MINGW* | MSYS* | CYGWIN*)
		host=windows
		arch=win64_msvc2022_64
		;;
	*) die "unsupported OS: $os" ;;
	esac
	echo "$host ${AQT_ARCH:-$arch}"
}

# Directory holding bin/, lib/ and lib/cmake of the installed Qt.
qt_dir() {
	local found
	found="$(find "$DEPS_DIR/Qt/$QT_VERSION" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
	[[ -n "$found" ]] || die "no Qt $QT_VERSION under $DEPS_DIR/Qt; run install-qt.sh first"
	echo "$found"
}
