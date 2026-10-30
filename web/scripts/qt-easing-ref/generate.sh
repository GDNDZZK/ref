#!/usr/bin/env bash
# Regenerates src/motion/__fixtures__/qt-easing.json from Qt's own easing source.
# Needs g++ and network access (fetches easing.cpp from the qtbase tag below).
set -euo pipefail

QT_TAG=v6.12.0
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../../src/motion/__fixtures__/qt-easing.json"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -fsSL -o "$work/easing.cpp" \
	"https://raw.githubusercontent.com/qt/qtbase/$QT_TAG/src/3rdparty/easing/easing.cpp"
g++ -std=c++17 -O0 -I"$here/shim" -I"$work" -o "$work/ref" "$here/easing_ref.cpp"
mkdir -p "$(dirname "$out")"
"$work/ref" > "$out"
echo "wrote $out (Qt $QT_TAG)"
