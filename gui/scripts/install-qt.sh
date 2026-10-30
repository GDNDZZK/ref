#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs Qt $QT_VERSION (see versions.sh) with aqtinstall into $REF_DEPS_DIR/Qt.
#
#   REF_DEPS_DIR=/path/to/deps gui/scripts/install-qt.sh
#
# Environment: REF_DEPS_DIR (default gui/.deps), AQT_ARCH (default: this machine's desktop arch).
# Windows on Arm also installs the x64 Qt: the Arm build runs its moc, rcc and qsb from there.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

read -r host arch < <(detect_aqt_target)

# Windows has python, not python3, unless the runner sets up both.
python="$(command -v python3 || command -v python)" || die "python3 not found"

venv="$DEPS_DIR/aqt-venv"
if [[ ! -d "$venv" ]]; then
	"$python" -m venv "$venv"
fi
venv_bin="$venv/bin"
[[ -d "$venv_bin" ]] || venv_bin="$venv/Scripts" # Windows layout
"$venv_bin/python" -m pip install --quiet "aqtinstall==$AQTINSTALL_VERSION"

aqt_options=()
if [[ "$host" == windows_arm64 ]]; then
	aqt_options+=(--autodesktop)
fi

# shellcheck disable=SC2086 # QT_MODULES is a word list
"$venv_bin/aqt" install-qt "$host" desktop "$QT_VERSION" "$arch" \
	--outputdir "$DEPS_DIR/Qt" \
	--modules $QT_MODULES \
	${aqt_options[@]+"${aqt_options[@]}"}

echo "Qt installed in $(qt_dir)"
