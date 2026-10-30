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

while IFS= read -r -d '' executable; do
  sign "$executable"
done < <(find "$APP/Contents/MacOS" -type f -print0)

sign "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
