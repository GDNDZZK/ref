# SPDX-License-Identifier: GPL-3.0-or-later
# Pinned dependency versions, sourced by install-qt.sh and build-deps.sh.
# Bump a version and its checksum together; build-deps.sh refuses a tarball that does not match.

QT_VERSION=6.12.0
AQTINSTALL_VERSION=3.3.0
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
