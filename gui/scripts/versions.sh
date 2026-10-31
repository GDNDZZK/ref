# SPDX-License-Identifier: GPL-3.0-or-later
# Pinned dependency versions, sourced by install-qt.sh and build-deps.sh.
# Bump a version and its checksum together; build-deps.sh refuses a tarball that does not match.

QT_VERSION=6.12.0
# aqtinstall 3.3.0, the latest release, cannot read the Windows package layout of Qt 6.11 and
# later ("Failed to locate XML data", miurahr/aqtinstall#959). The fix (PR #1000) is on master
# but not released yet, so this is a pinned commit of master. Go back to a version number
# ("aqtinstall==3.4.0" or later) once there is a release with that fix.
AQTINSTALL_SPEC="git+https://github.com/miurahr/aqtinstall.git@076e1659807d0b362a3ed684d54c2e9c775eb9c7"
# Qt modules beyond the base install (QtQuick, QtQuick.Controls, QtQuick.Shapes, QtQuick.Dialogs,
# QtNetwork, QtDBus and the Linguist tools come with it).
QT_MODULES="qtwebsockets qtshadertools"

# Extra CMake Modules and KSyntaxHighlighting come from the same KDE Frameworks release.
KF_VERSION=6.31.0
KF_BASE_URL="https://download.kde.org/stable/frameworks/${KF_VERSION%.*}"
ECM_SHA256=8f74af329e4c918358bfe54316f8cf4d530c28444595a3e2fb15075f72877f17
SYNTAX_HIGHLIGHTING_SHA256=7c2a64d3fd42ae433e9737b9cf6733b9ad3cb0e212285fd6820157d86831316c

QCORO_VERSION=0.13.0
QCORO_URL="https://github.com/danvratil/qcoro/archive/refs/tags/v${QCORO_VERSION}.tar.gz"
QCORO_SHA256=4bff7513c5c8e301b66308df05795043b1792ed16381a484e5c990171b8ff19e
