**English** · [简体中文](development.zh-CN.md)

# Development

## Building

You need the current stable Rust toolchain (the code uses edition 2024 and recent language features) and a C compiler, which the bundled SQLite and the TLS library need.

```bash
cargo build --release
./target/release/ref
```

Both programs end up in `target/release/`: `ref` and `ref-server`. Keep them in the same directory, because `ref` looks for `ref-server` next to itself first.

## Tests

```bash
cargo test --workspace                                         # all crates
cargo fmt --all --check                                        # formatting
cargo clippy --workspace --all-targets -- -D warnings          # lints
ref --mock                                                     # TUI against a fake backend
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
| `server` | `ref-server`: sessions, the agent loop, permissions, compaction, storage (SQLite), cross-session messaging, WebSocket API. |
| `tui` | `ref`: the terminal UI (ratatui). |

## Protocol notes

The TUI and the backend exchange JSON frames over WebSocket. Frontends get history page by page: `Attach` returns the newest messages (100, or fewer if they exceed about 1 MiB) and `Messages` returns the page before a given message, so the TUI loads older pages as you scroll up. Images reach frontends as stubs (MIME type, size and an `id`, no bytes); `GetImage` returns the bytes for an id.

## Continuous integration and releases

Two GitHub Actions workflows live in `.github/workflows/`:

- **`ci.yml`** runs on every push and pull request: `cargo fmt --check` and `cargo clippy -D warnings` on Linux, and `cargo test` on Linux, macOS and Windows. When tests fail, their output is repeated in the run summary.
- **`release.yml`** runs when a `v*` tag is pushed. It builds `ref` and `ref-server` for six targets on native runners, packs them with the READMEs, the `docs/` directory and the license, and publishes a GitHub Release with a `SHA256SUMS` file.

| Target | Runner | Archive |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64 | `.tar.gz` |
| `x86_64-apple-darwin` | macOS 15 Intel | `.tar.gz` |
| `aarch64-apple-darwin` | macOS 15 | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows Server 2025 | `.zip` |
| `aarch64-pc-windows-msvc` | Windows 11 arm64 | `.zip` |

To make a release:

```bash
# 1. Set the version in Cargo.toml ([workspace.package] version), commit and push.
# 2. Tag and push the tag:
git tag v0.2.0
git push origin v0.2.0
```

Running the Release workflow by hand (Actions → Release → Run workflow) only builds the archives and attaches them to the run, without publishing.
