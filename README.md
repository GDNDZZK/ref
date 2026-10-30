<div align="center">

# ref

**A terminal coding agent written in Rust.**

Sessions run in a background server and keep working while no window is open.
Several sessions can work side by side and message each other.

[![CI](https://github.com/GDNDZZK/ref/actions/workflows/ci.yml/badge.svg)](https://github.com/GDNDZZK/ref/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/GDNDZZK/ref?sort=semver)](https://github.com/GDNDZZK/ref/releases/latest)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey)

**English** · [简体中文](README.zh-CN.md)

</div>

---

## Features

- **Server and interfaces are separate.** `ref-server` runs sessions, the agent loop, LLM calls, tools and permissions. `ref` (terminal) and `ref-web` (browser) only show them. Close the window and the session keeps going; open it again from any terminal or browser, or in both at once.
- **Many sessions at once.** A split view tiles all background sessions. Sessions can send each other messages, and the `build` agent can start worker sessions and give them tasks.
- **Works with opencode configuration.** An existing `opencode.json` with providers, models, agents, permissions, MCP servers and commands works unchanged.
- **Any model provider.** Anthropic Messages, OpenAI Responses and OpenAI-compatible Chat Completions, with model variants for reasoning effort or extended thinking.
- **Control over tool calls.** Allow, ask or deny per tool, with patterns for commands, paths and URLs, plus optional countdowns before a tool runs.
- **Reliable file edits.** Edits address hash-tagged line numbers, so a stale view of a file is detected instead of corrupting it.
- **Long sessions.** Automatic context compaction, retries without limit for rate limits and outages, and image downscaling for every provider.
- **MCP** servers over stdio and Streamable HTTP.
- **Linux, macOS and Windows**, on x86_64 and arm64.

## Installation

### Download

Pick an interface, then get the archive for your system from the [latest release](https://github.com/GDNDZZK/ref/releases/latest). Every archive has the interface, `ref-server` (the backend), both READMEs, the license and `docs/`.

| Interface | Archive | Contents |
|---|---|---|
| Terminal (TUI) | `ref-tui-<version>-<platform>` | `ref`, `ref-server` |
| Browser (Web) | `ref-web-<version>-<platform>` | `ref-web`, `ref-server` |
| All | `ref-all-<version>-<platform>` | `ref`, `ref-web`, `ref-server` |

| System | `<platform>` and file type |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple silicon | `aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc.zip` |
| Windows arm64 | `aarch64-pc-windows-msvc.zip` |

For example, `ref-tui-v0.2.0-x86_64-unknown-linux-gnu.tar.gz`. Unpack it and put the programs **in the same directory** on your `PATH`; each interface looks for `ref-server` next to itself:

```bash
tar -xzf ref-tui-*.tar.gz
install -m 755 ref-tui-*/ref ref-tui-*/ref-server ~/.local/bin/
```

All interfaces share one backend. If you installed several, any of them connects to the backend that is already running.

- **Linux**: needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36, RHEL 10, Arch and newer). NixOS needs `programs.nix-ld.enable = true`.
- **macOS**: the binaries are not signed. If macOS blocks them, run `xattr -d com.apple.quarantine ref ref-web ref-server` (the names that are in your archive).
- **Windows**: install [Git for Windows](https://git-scm.com/download/win); the `bash` tool uses its Git Bash.

Each release has a `SHA256SUMS` file for checking the download.

### Build from source

```bash
git clone https://github.com/GDNDZZK/ref.git
cd ref
(cd web && pnpm install && pnpm build)   # needs Node 24 and pnpm; ref-web embeds the result
cargo build --release                    # needs current stable Rust and a C compiler
./target/release/ref
```

## Quick start

1. Set an API key, for example `export ANTHROPIC_API_KEY=...`. `OPENAI_API_KEY`, `DEEPSEEK_API_KEY` and `OPENROUTER_API_KEY` work too. For other providers, see [Configuration](docs/configuration.md#providers-and-models).
2. Run `ref` in your project directory.
3. Type a task and press `enter`.

To use the browser instead, run `ref-web` in your project directory; it opens the page. See [Web UI](docs/web.md).

Keys in the terminal UI:

| Key | Action |
|---|---|
| `ctrl+x m` / `ctrl+x a` | Pick a model / an agent |
| `tab` | Next agent (`build` edits, `plan` only reads) |
| `ctrl+x l` | Sessions, live and past |
| `ctrl+x w` | Split view of background sessions |
| `esc esc` | Interrupt |
| `ctrl+x h` | All keys and commands |

## Documentation

| Document | Contents |
|---|---|
| [Using ref](docs/usage.md) | Sessions, the screen, prompts, keybindings, slash commands, split view, working with several sessions |
| [Configuration](docs/configuration.md) | Every configuration key: providers and models, agents, permissions, delays, compaction, MCP, commands, instructions and skills |
| [Tools](docs/tools.md) | What the model can do: file tools, bash, webfetch, image handling, context and cross-session tools |
| [Web UI](docs/web.md) | `ref-web`: starting, signing in, opening it to the network, developing the page |
| [Command line](docs/cli.md) | `ref`, `ref-web` and `ref-server` options, remote backends, files and environment variables |
| [Development](docs/development.md) | Building, tests, crate layout, CI and releases |

## License

[GPL-3.0](LICENSE)
