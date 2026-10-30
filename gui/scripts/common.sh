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
# override the arch (e.g. win64_msvc2022_arm64, which Git Bash on an Arm runner cannot report
# reliably). aqt names the host of Arm machines linux_arm64 / windows_arm64.
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
	arch="${AQT_ARCH:-$arch}"
	if [[ "$arch" == *arm64* ]]; then
		host="${host}_arm64"
	fi
	echo "$host $arch"
}

# Name of the directory aqt creates under $DEPS_DIR/Qt/$QT_VERSION for an aqt arch.
qt_arch_dir() {
	case "$1" in
	clang_64) echo macos ;;
	linux_*) echo "${1#linux_}" ;;
	win64_*) echo "${1#win64_}" ;;
	*) die "unknown aqt arch: $1" ;;
	esac
}

# Directory holding bin/, lib/ and lib/cmake of the installed Qt for this machine's arch.
qt_dir() {
	local host arch dir
	read -r host arch < <(detect_aqt_target)
	dir="$DEPS_DIR/Qt/$QT_VERSION/$(qt_arch_dir "$arch")"
	[[ -d "$dir" ]] || die "no Qt $QT_VERSION in $dir; run install-qt.sh first"
	echo "$dir"
}

# Directory of the Qt for the build machine, for cross builds that need its tools
# (QT_HOST_PATH). Only Windows on Arm installs a second Qt for that; prints nothing elsewhere.
qt_host_dir() {
	local host arch
	read -r host arch < <(detect_aqt_target)
	[[ "$host" == windows_arm64 ]] || return 0
	echo "$DEPS_DIR/Qt/$QT_VERSION/msvc2022_64"
}
