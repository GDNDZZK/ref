#!/usr/bin/env bash
# Signs a macOS app bundle ad hoc, inside out, and verifies the result.
#
#   scripts/sign-macos-app.sh ref.app
#
# macdeployqt signs what it copies, but the bundle is changed afterwards (ref-server is added),
# and a signature covers everything inside a bundle, so every layer is signed again: first the
# libraries and plugins, then the nested bundles (frameworks), then the executables, last the
# app. There is no developer identity, hence "-" (ad hoc). Users still have to clear the
# quarantine flag, see the README.

set -euo pipefail

APP="${1:?usage: $0 APP.app}"
[[ -d "$APP/Contents/MacOS" ]] || { echo "error: $APP is not an app bundle" >&2; exit 1; }

sign() {
  codesign --force --sign - --timestamp=none "$1"
}

# -depth lists a directory's content before the directory itself. Symlinks are skipped:
# the file they point to is signed on its own.
while IFS= read -r -d '' item; do
  sign "$item"
done < <(find "$APP/Contents" -depth ! -type l \
  \( -name '*.dylib' -o -name '*.so' -o -name '*.framework' \) -print0)

# The main executable goes last: codesign checks the other programs in the folder when it signs
# it, and fails with "code object is not signed at all" for ref-server if that is not signed yet.
# (On Apple Silicon the linker signs every program it makes, so only Intel hits this.)
main_executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
while IFS= read -r -d '' executable; do
  sign "$executable"
done < <(find "$APP/Contents/MacOS" -type f ! -name "$main_executable" -print0)
sign "$APP/Contents/MacOS/$main_executable"

sign "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
