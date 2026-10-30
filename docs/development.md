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
(cd web && pnpm check)                                         # frontend: types, lint, unit tests, generated files up to date
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

The TUI and the backend exchange JSON frames over WebSocket. Frontends get history page by page: `Attach` returns the newest messages (100, or fewer if they exceed about 1 MiB) and `Messages` returns the page before a given message, so the TUI loads older pages as you scroll up. Images reach frontends as stubs (MIME type, size and an `id`, no bytes); `GetImage` returns the bytes for an id.

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
