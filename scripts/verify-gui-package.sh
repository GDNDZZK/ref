#!/usr/bin/env bash
# Checks that an unpacked ref-gui package is complete and runs, on the platform it was built for.
#
#   scripts/verify-gui-package.sh ref-gui-v0.2.0-x86_64-unknown-linux-gnu
#
# - Every library the program and its plugins need is in the package or part of the system.
# - Linux: the package asks no more of the system than the limits below.
# - macOS: the signature is valid and nothing refers to a path outside the bundle.
# - Windows: the program uses the GUI subsystem, so it opens no console window.
# - The program starts against the ref-server in the package and opens a session on a mock
#   backend (`--smoke --mock`), which loads the QML, the platform plugin and the QML modules.

set -euo pipefail

# What the package may ask of the system: glibc 2.38 and the libstdc++ of GCC 13.
# C++23 needs GCC 13, so the GUI is built on Ubuntu 24.04. Its headers make strtol and friends
# resolve to the glibc 2.38 symbols (__isoc23_*), and the arm64 Qt from aqtinstall is built
# on Ubuntu 24.04 and needs 2.38 too. Older libstdc++ versions lack CXXABI_1.3.14, so Debian 12
# and Ubuntu 22.04 cannot run the GUI. README.md and docs/development.md state the same numbers.
MAX_GLIBC=2.38
MAX_GLIBCXX=3.4.32
MAX_CXXABI=1.3.14
SMOKE_TIMEOUT_SECONDS=60
SYNTAX_HIGHLIGHTING_QMLDIR=org/kde/syntaxhighlighting/qmldir
PE_SUBSYSTEM_GUI=2
# Paths in macOS libraries that only exist on the build machine.
MACOS_FOREIGN_PATH_REGEX='^[[:space:]]*(/Users|/opt|/usr/local|/Applications|/Volumes|/private|/home|/Library)/'

PACKAGE="${1:?usage: $0 PACKAGE_DIR}"
PACKAGE="$(cd "$PACKAGE" && pwd)"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

pass() { echo "ok: $*"; }

# Prints the paths of the files in the package that match find's arguments.
files() { find "$PACKAGE" -type f "$@"; }

# Succeeds when version $1 is at most $2 (dotted numbers).
version_at_most() {
  [[ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" == "$2" ]]
}

check_syntax_highlighting_module() {
  [[ -n "$(files -path "*/$SYNTAX_HIGHLIGHTING_QMLDIR")" ]] || fail "the KSyntaxHighlighting QML module is missing"
  pass "KSyntaxHighlighting QML module"
}

# ref-gui uses the Basic style only (gui/cmake/PruneControlsStyles.cmake removes the others),
# but still needs the Basic style and what QtQuick.Dialogs loads.
check_controls_styles() {
  local other
  other="$(find "$PACKAGE" -path '*/QtQuick/Controls/*' -maxdepth 8 -type d -prune \
    ! -name Basic ! -name impl -print | grep -E '/QtQuick/Controls/[^/]+$' || true)"
  [[ -z "$other" ]] || fail "styles other than Basic are in the package: $other"

  other="$(find "$PACKAGE" -iname '*QuickControls2*' ! -path '*/QtQuick/Controls/*' |
    grep -E 'QuickControls2(Fusion|Imagine|Material|Universal|FluentWinUI3|Windows|MacOS|IOS)' || true)"
  [[ -z "$other" ]] || fail "libraries of other styles are in the package: $other"

  [[ -n "$(files -path '*/QtQuick/Controls/Basic/qmldir')" ]] || fail "the Basic style is missing"
  [[ -n "$(files -path '*/QtQuick/Dialogs/qmldir')" ]] || fail "the QtQuick.Dialogs module is missing"
  pass "Controls styles: Basic only"
}

# Runs the program the way a user would, with the environment of the build machine removed.
smoke() {
  local program=$1 output status=0
  shift
  output="$("$@" "$program" --smoke --mock 2>&1)" || status=$?
  echo "$output"
  [[ $status -eq 0 ]] || fail "$program --smoke --mock exited with $status"
  pass "$program --smoke --mock"
}

verify_linux() {
  local elf missing highest name limit_var
  local clean_env=(env -u LD_LIBRARY_PATH -u QML_IMPORT_PATH -u QML2_IMPORT_PATH -u QT_PLUGIN_PATH
    -u WAYLAND_DISPLAY)

  while IFS= read -r elf; do
    missing="$("${clean_env[@]}" ldd "$elf" 2>&1 | grep 'not found' || true)"
    [[ -z "$missing" ]] || fail "$elf: $missing"
  done < <(files \( -name '*.so' -o -name '*.so.*' -o -name ref-gui -o -name ref-server \))
  pass "all libraries resolve"

  # Per file, so that the message names the file that asks too much.
  for name in GLIBC GLIBCXX CXXABI; do
    limit_var="MAX_$name"
    while IFS= read -r elf; do
      # grep finds nothing in a file that uses no versioned symbol of this kind.
      highest="$({ objdump -T "$elf" 2>/dev/null || true; } | { grep -o "${name}_[0-9.]*" || true; } | sed "s/^${name}_//" | sort -uV | tail -n 1)"
      version_at_most "${highest:-0}" "${!limit_var}" || fail "$elf needs $name $highest, at most ${!limit_var} is allowed"
    done < <(files \( -name '*.so' -o -name '*.so.*' -o -name ref-gui -o -name ref-server \))
    pass "$name <= ${!limit_var}"
  done

  check_syntax_highlighting_module
  check_controls_styles
  smoke "$PACKAGE/ref-gui" xvfb-run -a "${clean_env[@]}"
}

verify_macos() {
  local app="$PACKAGE/ref.app" binary foreign
  [[ -d "$app" ]] || fail "$app is missing"
  [[ -x "$app/Contents/MacOS/ref-server" ]] || fail "ref-server is missing from the app"

  codesign --verify --deep --strict "$app" || fail "the signature is not valid"
  pass "signature"

  while IFS= read -r binary; do
    # Qt's plugins are universal binaries: otool prints "<file> (architecture ...):" before each
    # slice's libraries, and those header lines hold the path of the file itself.
    foreign="$(otool -L "$binary" 2>/dev/null | grep -v ':$' | grep -E "$MACOS_FOREIGN_PATH_REGEX" || true)"
    [[ -z "$foreign" ]] || fail "$binary refers to the build machine: $foreign"
  done < <(find "$app" -type f \( -name '*.dylib' -o -name '*.so' -o -path '*/Contents/MacOS/*' -o -path '*.framework/Versions/*/*' \) ! -name '*.plist')
  pass "no references outside the bundle"

  check_syntax_highlighting_module
  check_controls_styles
  smoke "$app/Contents/MacOS/ref-gui" env
}

# The subsystem field of the PE header: 2 is a GUI program, 3 a console program.
pe_subsystem() {
  local exe=$1 pe_offset
  pe_offset="$(od -An -t u4 -j 60 -N 4 "$exe" | tr -d ' ')"
  od -An -t u2 -j $((pe_offset + 24 + 68)) -N 2 "$exe" | tr -d ' '
}

verify_windows() {
  local exe="$PACKAGE/ref-gui.exe" required
  [[ -f "$exe" ]] || fail "$exe is missing"
  [[ "$(pe_subsystem "$exe")" == "$PE_SUBSYSTEM_GUI" ]] || fail "ref-gui.exe is a console program"
  pass "GUI subsystem"

  for required in ref-server.exe Qt6Core.dll Qt6Quick.dll platforms/qwindows.dll; do
    [[ -f "$PACKAGE/$required" ]] || fail "$required is missing"
  done
  [[ -n "$(files -iname 'vcruntime140*.dll')" ]] || fail "the Visual C++ runtime is missing"
  [[ -n "$(files -iname 'KF6SyntaxHighlighting.dll')" ]] || fail "KF6SyntaxHighlighting.dll is missing"
  pass "required files"

  check_syntax_highlighting_module
  check_controls_styles
  smoke "$exe" env
}

case "$(uname -s)" in
  Linux) verify_linux ;;
  Darwin) verify_macos ;;
  MINGW* | MSYS* | CYGWIN*) verify_windows ;;
  *) fail "unsupported system $(uname -s)" ;;
esac
