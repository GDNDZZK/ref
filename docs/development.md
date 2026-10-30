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

## Continuous integration and releases

Two GitHub Actions workflows live in `.github/workflows/`. Both build the frontend first with the actions in `.github/actions/` (Node 24, pnpm with a cached store, `pnpm install --frozen-lockfile`, `pnpm build`).

- **`ci.yml`** runs on every push and pull request: `cargo fmt --check` and `cargo clippy -D warnings` on Linux, `cargo test` on Linux, macOS and Windows, and `pnpm check` for the frontend. When tests or the frontend check fail, their output is repeated in the run summary.
- **`release.yml`** runs when a `v*` tag is pushed. It builds the frontend once, then builds `ref`, `ref-server` and `ref-web` for six targets on native runners (one `cargo build --release --workspace` per target) and packs them with `scripts/package.sh`. A final job writes the `SHA256SUMS` file and, for tags, publishes a GitHub Release. Every build lists the archive sizes in the run summary.

| Target | Runner | Archive |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64 | `.tar.gz` |
| `x86_64-apple-darwin` | macOS 15 Intel | `.tar.gz` |
| `aarch64-apple-darwin` | macOS 15 | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows Server 2025 | `.zip` |
| `aarch64-pc-windows-msvc` | Windows 11 arm64 | `.zip` |

Each target gets three archives. Every archive holds one folder of the same name with the interface, `ref-server`, both READMEs, the license and `docs/`.

| Archive | Programs |
|---|---|
| `ref-tui-<version>-<target>` | `ref`, `ref-server` |
| `ref-web-<version>-<target>` | `ref-web`, `ref-server` |
| `ref-all-<version>-<target>` | `ref`, `ref-web`, `ref-server` |

To pack by hand after `cargo build --release`:

```bash
scripts/package.sh --version v0.2.0 --target x86_64-unknown-linux-gnu \
  --bin-dir target/release --out dist
```

To make a release:

```bash
# 1. Set the version in Cargo.toml ([workspace.package] version), commit and push.
# 2. Tag and push the tag:
git tag v0.2.0
git push origin v0.2.0
```

Running the Release workflow by hand (Actions → Release → Run workflow) builds the archives and `SHA256SUMS` and attaches them to the run, without publishing.
