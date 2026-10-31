**English** · [简体中文](development.zh-CN.md)

# Development

## Building

You need the current stable Rust toolchain (the code uses edition 2024 and recent language features), a C compiler, which the bundled SQLite and the TLS library need, and [Node.js](https://nodejs.org/) 24 with [pnpm](https://pnpm.io/) for the web frontend.

**Build the frontend first.** `ref-web` embeds the files in `web/dist/`, and its build script stops with an error when `web/dist/index.html` is missing. This applies to every cargo command that compiles the workspace: `cargo build`, `cargo test`, `cargo clippy --workspace`.

```bash
(cd web && pnpm install && pnpm build)
cargo build --release
./target/release/ref
```

The programs end up in `target/release/`: `ref`, `ref-web` and `ref-server`. Keep each interface next to `ref-server`, because they look for it in their own directory first. Run `pnpm build` again after changing the frontend, then rebuild `ref-web`; cargo notices changes in `web/dist/`.

## Tests

```bash
cargo test --workspace                                         # all crates (needs web/dist, see above)
cargo fmt --all --check                                        # formatting
cargo clippy --workspace --all-targets -- -D warnings          # lints
ref --mock                                                     # TUI against a fake backend
(cd web && pnpm check)                                         # frontend: types, lint, unit tests, generated Theme/Motion files up to date
python3 scripts/e2e.py provider/model "prompt" [cwd]           # end-to-end against a real model
REF_DUMP_REQUESTS=/tmp/dump ref-server ...                     # dump LLM request bodies
```

The `ref-web` tests that start a backend use the `ref-server` binary next to `ref-web` and fail with a hint when it is missing. `cargo test --workspace` builds it; with `cargo test -p ref-web`, run `cargo build -p ref-server` first.

Some tests start `python3` (a fake MCP server) and `bash`. The end-to-end scripts need the Python `websockets` package and a configured model.

## Crates

| Crate | Contents |
|---|---|
| `proto` | Wire protocol between the TUI and the backend, shared data types and directory paths. |
| `config` | Loading and merging configuration (opencode-compatible). |
| `llm` | Streaming clients for OpenAI Chat Completions, OpenAI Responses and Anthropic Messages; error classification and retries. |
| `tools` | Built-in tools: `read`, `edit`, `write`, `delete`, `grep`, `find`, `bash`, `webfetch`, image fitting. |
| `mcp` | MCP client over stdio and Streamable HTTP. |
| `sys` | Operating-system layer: processes, signals, Windows specifics. |
| `client` | Finds the running backend through the instance files or starts the `ref-server` next to the program, or a private `ref-server --mock`. Shared by `ref` and `ref-web`. |
| `server` | `ref-server`: sessions, the agent loop, permissions, compaction, storage (SQLite), cross-session messaging, WebSocket API. |
| `tui` | `ref`: the terminal UI (ratatui). |
| `web` | `ref-web`: serves the web frontend from `web/` (embedded, pre-compressed), signs browsers in and forwards their WebSocket frames to `ref-server`. See [Web UI](web.md). |

## Protocol notes

The protocol types are in `crates/proto`. Two sets of files are generated from them and checked into the repository: JSON samples of every frame in `crates/proto/fixtures/` (the GUI and the web frontend parse each one in their tests) and the TypeScript types in `web/src/proto/generated/`. `cargo test -p ref-proto` fails when they are out of date; `REF_UPDATE_FIXTURES=1 cargo test -p ref-proto` rewrites them. Both directories are kept on LF line endings by `.gitattributes`.

The TUI and the backend exchange JSON frames over WebSocket. Frontends get history page by page: `Attach` returns the newest messages (100, or fewer if they exceed about 1 MiB) and `Messages` returns the page before a given message, so the TUI loads older pages as you scroll up. Images reach frontends as stubs (MIME type, size and an `id`, no bytes); `GetImage` returns the bytes for an id.

## The mock backend

`ref-server --mock` is a fake backend for developing interfaces without an LLM. `ref-gui --mock` and `ref-web --mock` start a private one through `ref-client`. (`ref --mock` is a separate, in-process fake and does not use it.)

To run one by hand, keep its stdin open, because it exits when stdin reaches end of file:

```bash
sleep 3600 | ref-server --mock      # prints {"port":N,"token":"..."} on stdout, nothing else
```

- **Exit**: stdin end of file, the last client disconnecting, or ctrl-c. It then deletes its temporary directory (a `SIGKILL` leaves `/tmp/ref-mock-*` behind).
- **Workspace**: every session runs in a temporary workspace with sample files (`README.md`, `NOTES.txt`, `src/parser.rs`, `src/lib.rs`, `docs/design.md`, `data/big.log`, `logo.png`, `scratch/`), whatever directory the client asks for. Your files are never touched. Three past sessions are preloaded as history.
- **Models**: `mock/mock-1` (default; 200k context, images, reasoning, variants `low`, `medium`, `high`) and `mock/mock-fast` (64k). There is an extra agent, `reviewer`.
- **Permissions**: `edit` and `write` ask (allowed in `scratch/`). `delete` is allowed after a 5-second delay. `bash` asks, except commands starting with `echo`, `ls`, `cat`, `seq`, `sleep` or `printf`, which are allowed, and `curl`, which is denied. `webfetch` is off.
- **Limits**: the mock does not block everything. *Export* writes to the path the client gives, and a `!` shell command runs a real command, so both can reach outside the workspace. Tools that the scripts call only touch the workspace. The directory picker is limited to the workspace, where `~` means the workspace.
- **Scripts**: the first word of a prompt picks a script, case-insensitively. Other text runs `default`. The prompt `help` lists them.

| Prompt | What happens |
|---|---|
| `help` | Lists all scripts |
| `hello` | A one-sentence reply |
| `default` | Any other text: echoes the prompt |
| `markdown` | Headings, lists, a table, a quote, links, code blocks in several languages |
| `reasoning` | Long reasoning, then a short answer |
| `article` | A long article with many code blocks (about 18 s) |
| `slow` | Very slow streaming, about a minute, for frame-by-frame checks |
| `tools` | `read`, reading a missing file (error), `grep`, `find`, an `edit` that needs approval (with diff), `bash` succeeding and exiting with 3 |
| `stream-args` | A `write` whose arguments stream slowly |
| `permission` | An `edit` waiting for approval: `a` allow, `s` allow for the session, `d` deny |
| `delay` | A `delete` with a 5-second countdown (`esc` cancels) |
| `running` | A `bash` command that prints for 8 seconds |
| `bash-fail` | A `bash` command that exits with 1 |
| `denied` | A call denied by the configuration |
| `cancel` | A command that never ends, for cancelling with `esc` |
| `todo` | A todo list updated in several steps |
| `image` | Reads `logo.png`; the tool result has an image |
| `long` | A tool with 400 lines of output, then a long answer |
| `retry` | Two 529 errors (retrying), then success (about 6 s) |
| `fail` | A 401 error; retries, then ends with the error (about 6 s) |
| `fill` | Context near 95%; the next prompt compacts first |
| `compact-tool` | The model calls `compact_context` |
| `peer` | Creates a `reviewer` session `helper`, which reports back with `send_message` |
| `everything` | `tools`, `todo` and `image` in one long turn |
| `orchestrate` | The main session plans, creates the helpers `scout`, `fixer`, `tester` and `docs`, collects their five reports (the fifth from a `reviewer` it creates last) and ends with a summary (about 25 s) |

`orchestrate` takes about 25 seconds and needs one approval, for the fixer's `edit`. The timeline is fixed: the helpers appear at 1.5, 3, 4.5 and 6 s, the main session is idle at about 7.5 s, the reports arrive at about 10.5 (`scout`), 12 (`docs`), 12.5 (`fixer`, 3.5 s after you approve at 9 s) and 14 s (`tester`), the reviewer is created at 16 s, reports at 21 s, and the summary ends at 25 s. A report that arrives while the main session is busy joins the conversation at its next request and is counted all the same. The words in the prompt after `orchestrate` are not used. The other scripts it starts (`orch-scout`, `orch-fixer`, `orch-tester`, `orch-docs`, `orch-review`, `orchestrate-report`) are listed by `help` but are not meant to be typed.

## The GUI

`ref-gui` is in `gui/` (C++23, Qt Quick, CMake). It is built apart from the Rust workspace. You need a C++23 compiler (GCC 13, a current Clang or Visual Studio 2022), CMake 3.25 and Ninja. The libraries are installed by two scripts in `gui/scripts/` that CI runs too; the versions are pinned in `gui/scripts/versions.sh` (Qt 6.12.0 from the official binary packages through aqtinstall; extra-cmake-modules, KSyntaxHighlighting and QCoro built from source):

```bash
export REF_DEPS_DIR=$HOME/ref-deps                 # default: gui/.deps
gui/scripts/install-qt.sh
gui/scripts/build-deps.sh
cargo build -p ref-server                          # two tests run against the real backend
cmake -S gui -B gui/build -G Ninja \
  -DCMAKE_PREFIX_PATH="$(gui/scripts/qt-dir.sh);$REF_DEPS_DIR/prefix" \
  -DREF_SERVER=$PWD/target/debug/ref-server
cmake --build gui/build
QT_QPA_PLATFORM=offscreen QML_IMPORT_PATH=$REF_DEPS_DIR/prefix/lib/qml \
  ctest --test-dir gui/build --output-on-failure
```

`ref-gui` loads KSyntaxHighlighting's QML module at run time. It sits in `$REF_DEPS_DIR/prefix/lib/qml`, so that directory must be in `QML_IMPORT_PATH` when you run the program from the build folder. A deployed package carries the module itself.

### Deploying

`gui/cmake/Deploy.cmake` holds the install rules. `cmake --install` copies the program and, with Qt's deploy API, the Qt libraries, plugins and QML modules it needs. The QML modules are found by scanning the QML sources for imports, so a new import is deployed without a change to a list. Configure with `-DCMAKE_INSTALL_BINDIR=.` to get a flat folder with `ref-gui` at the top:

```bash
cmake -S gui -B gui/build -G Ninja -DCMAKE_INSTALL_BINDIR=. -DCMAKE_PREFIX_PATH=… 
cmake --build gui/build
cmake --install gui/build --prefix stage
scripts/package.sh --version v0.2.0 --target x86_64-unknown-linux-gnu \
  --bin-dir target/release --out dist --gui-dir stage      # needs the Rust build, see above
```

`cmake --install` also removes the Qt Quick Controls styles other than Basic (`gui/cmake/PruneControlsStyles.cmake`): `ref-gui` sets the Basic style, and the scan for QML imports would otherwise deploy all five.

- **Linux**: libraries go in `lib/` and the program has the run path `$ORIGIN/lib`; `qt.conf` points Qt to `plugins/` and `qml/`. Plugins that a desktop does not use (QML debugging, embedded displays, Wayland helpers) are left out.
- **macOS**: `macdeployqt` builds `ref-gui.app`. `package.sh` renames it to `ref.app`, puts `ref-server` into `Contents/MacOS` and runs `scripts/sign-macos-app.sh`, which signs everything again, inner files first, with an ad-hoc signature (there is no developer certificate) and checks it with `codesign --verify --deep --strict`. The `all` archive links its `ref-server` to the one in the app.
- **Windows**: `windeployqt` copies the DLLs and the Visual C++ runtime next to `ref-gui.exe`, without the software OpenGL fallback. The program uses the GUI subsystem (no console window) and has an icon and version resource.

`scripts/verify-gui-package.sh DIR` checks an unpacked `ref-gui` archive and runs on the platform it was built for. CI runs it after packing. It checks that every library resolves without the build machine's environment, that the KSyntaxHighlighting module is there, that the only Controls style is Basic, and that `ref-gui --smoke --mock` opens a session (on Linux under `xvfb-run`). On Linux it also checks what the package asks of the system, and on macOS the signature and that nothing refers to a path outside the app. On Windows it checks that `ref-gui.exe` uses the GUI subsystem.

**Linux requirements.** C++23 needs GCC 13, which Ubuntu 22.04 does not have, so CI builds the Linux GUI on Ubuntu 24.04 while `ref`, `ref-web` and `ref-server` are still built on 22.04. The program built there needs glibc 2.38: the Ubuntu 24.04 headers turn `strtol` and similar calls into the `__isoc23_*` symbols that glibc 2.38 introduced, and the arm64 Qt that aqtinstall installs is built on Ubuntu 24.04 and needs 2.38 as well (the x86-64 Qt needs 2.34 at most). Building the GUI against an older glibc would need a different toolchain and Qt build, which the project does not have. A program built with GCC 13 needs `CXXABI_1.3.14` from `libstdc++`, which the GCC 12 of Ubuntu 22.04 and Debian 12 does not have. `verify-gui-package.sh` therefore fails when anything in the package needs more than glibc 2.38, `GLIBCXX_3.4.32` or `CXXABI_1.3.14`; the README states the same numbers. If a newer compiler or Qt raises one, change the constants at the top of the script and the README together.

## Continuous integration and releases

Two GitHub Actions workflows live in `.github/workflows/`. Both build the frontend first with the actions in `.github/actions/` (Node 24, pnpm with a cached store, `pnpm install --frozen-lockfile`, `pnpm build`).

- **`ci.yml`** runs on every push and pull request: `cargo fmt --check` and `cargo clippy -D warnings` on Linux, `cargo test` on Linux, macOS (Intel) and Windows, `pnpm check` for the frontend, and the GUI build with its tests (`ctest`) on the same three systems. The GUI libraries come from `gui/scripts` and are cached by the hash of those scripts. When tests or the frontend check fail, their output is repeated in the run summary.
- **`release.yml`** runs when a `v*` tag is pushed. Its jobs, in order: build the frontend once; build `ref`, `ref-server` and `ref-web` for six targets on native runners (one `cargo build --release --workspace` per target); build and deploy the GUI for each target and pack the four archives with `scripts/package.sh`, then check the GUI archive with `scripts/verify-gui-package.sh`; write the `SHA256SUMS` file and, for tags, publish a GitHub Release. Every package job lists the archive sizes in the run summary.

| Target | Runner (Linux GUI and packaging) | Archive |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 (24.04) | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64 (24.04 arm64) | `.tar.gz` |
| `x86_64-apple-darwin` | macOS 15 Intel | `.tar.gz` |
| `aarch64-apple-darwin` | macOS 15 | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows Server 2025 | `.zip` |
| `aarch64-pc-windows-msvc` | Windows 11 arm64 | `.zip` |

Each target gets four archives. Every archive holds one folder of the same name with the interface, `ref-server`, both READMEs, the license and `docs/`.

| Archive | Programs |
|---|---|
| `ref-tui-<version>-<target>` | `ref`, `ref-server` |
| `ref-gui-<version>-<target>` | the GUI (see [Deploying](#deploying)), `ref-server` |
| `ref-web-<version>-<target>` | `ref-web`, `ref-server` |
| `ref-all-<version>-<target>` | `ref`, `ref-web`, the GUI, `ref-server` |

To pack by hand after `cargo build --release`:

```bash
scripts/package.sh --version v0.2.0 --target x86_64-unknown-linux-gnu \
  --bin-dir target/release --out dist [--gui-dir stage]
```

Without `--gui-dir` (the folder from `cmake --install`, see [Deploying](#deploying)) no `gui` archive is made and `all` has no GUI.

To make a release:

```bash
# 1. Set the version in Cargo.toml ([workspace.package] version) and run
#    `cargo update --workspace` to refresh Cargo.lock (CI builds with --locked).
#    Write the release text in .github/release-notes/v0.2.0.md. Commit and push.
# 2. Tag and push the tag:
git tag v0.2.0
git push origin v0.2.0
```

The `ref-gui` version, the Windows and macOS metadata and the archive names follow from the tag and `Cargo.toml`. The first job of the workflow fails when the tag and the version in `Cargo.toml` differ. The release text is the file `.github/release-notes/<tag>.md`; when it does not exist, GitHub generates the text from the commits.

Running the Release workflow by hand (Actions → Release → Run workflow) builds the archives and `SHA256SUMS` and attaches them to the run, without publishing.
