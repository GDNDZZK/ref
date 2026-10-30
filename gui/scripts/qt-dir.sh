#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prints the directory of the installed Qt (the value for CMAKE_PREFIX_PATH), or with --host
# the Qt for the build machine (QT_HOST_PATH; Windows on Arm only).
#
#   gui/scripts/qt-dir.sh [--host]
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

if [[ "${1:-}" == --host ]]; then
	qt_host_dir
else
	qt_dir
fi
