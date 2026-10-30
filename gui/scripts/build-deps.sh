#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Builds extra-cmake-modules, KSyntaxHighlighting and QCoro from source against the Qt that
# install-qt.sh installed, and installs them into $REF_DEPS_DIR/prefix.
#
#   REF_DEPS_DIR=/path/to/deps gui/scripts/build-deps.sh
#
# Environment: REF_DEPS_DIR (default gui/.deps), CMAKE_GENERATOR (default Ninja),
# BUILD_TYPE (default Release).
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

qt="$(qt_dir)"
prefix="$DEPS_DIR/prefix"
work="$DEPS_DIR/work"
build_type="${BUILD_TYPE:-Release}"
mkdir -p "$prefix" "$work"

# download URL SHA256 -> path of the verified tarball
download() {
	local url="$1" sha256="$2" file
	file="$work/$(basename "$url")"
	if [[ ! -f "$file" ]]; then
		curl --fail --location --silent --show-error --output "$file" "$url"
	fi
	echo "$sha256  $file" | sha256sum --check --status || die "checksum mismatch for $file"
	echo "$file"
}

# build_cmake_project NAME TARBALL [cmake args...]
build_cmake_project() {
	local name="$1" tarball="$2"
	shift 2
	local src="$work/src-$name" build="$work/build-$name"
	rm -rf "$src" "$build"
	mkdir -p "$src"
	tar -xf "$tarball" -C "$src" --strip-components=1

	cmake -S "$src" -B "$build" -G "${CMAKE_GENERATOR:-Ninja}" \
		-DCMAKE_BUILD_TYPE="$build_type" \
		-DCMAKE_INSTALL_PREFIX="$prefix" \
		-DCMAKE_INSTALL_LIBDIR=lib \
		-DCMAKE_PREFIX_PATH="$qt;$prefix" \
		-DBUILD_TESTING=OFF \
		"$@"
	cmake --build "$build" --config "$build_type"
	cmake --install "$build" --config "$build_type"
}

ecm_tarball="$(download "$KF_BASE_URL/extra-cmake-modules-$KF_VERSION.tar.xz" "$ECM_SHA256")"
syntax_tarball="$(download "$KF_BASE_URL/syntax-highlighting-$KF_VERSION.tar.xz" "$SYNTAX_HIGHLIGHTING_SHA256")"
qcoro_tarball="$(download "$QCORO_URL" "$QCORO_SHA256")"

build_cmake_project extra-cmake-modules "$ecm_tarball" -DBUILD_DOC=OFF -DBUILD_HTML_DOCS=OFF -DBUILD_MAN_DOCS=OFF
build_cmake_project syntax-highlighting "$syntax_tarball" -DBUILD_QCH=OFF -DKDE_INSTALL_USE_QT_SYS_PATHS=OFF -DKDE_INSTALL_LIBDIR=lib
build_cmake_project qcoro "$qcoro_tarball" \
	-DUSE_QT_VERSION=6 \
	-DQCORO_BUILD_EXAMPLES=OFF \
	-DQCORO_BUILD_TESTING=OFF \
	-DQCORO_WITH_QTDBUS=OFF \
	-DQCORO_WITH_QTNETWORK=ON \
	-DQCORO_WITH_QTWEBSOCKETS=ON \
	-DQCORO_WITH_QTQUICK=OFF \
	-DQCORO_WITH_QML=OFF \
	-DQCORO_WITH_QTTEST=OFF

cat <<MSG

Dependencies installed. Configure ref-gui with:
  cmake -S <gui dir> -B <build dir> -G Ninja -DCMAKE_PREFIX_PATH="$qt;$prefix"
At run time, QML_IMPORT_PATH / QML2_IMPORT_PATH must include $prefix/lib/qml
(KSyntaxHighlighting's QML plugin).
MSG
