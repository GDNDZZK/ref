**English** · [简体中文](cli.zh-CN.md)

# Command line

- [`ref`](#ref)
- [`ref-web`](#ref-web)
- [`ref-server`](#ref-server)
- [Running the backend on another machine](#running-the-backend-on-another-machine)
- [Files and directories](#files-and-directories)
- [Environment variables](#environment-variables)

## `ref`

The terminal UI.

```
ref [PATH] [options]
```

| Option | Meaning |
|---|---|
| `PATH` | Directory for the new session (default: the current directory). |
| `-c`, `--continue` | Continue the most recent session for this directory. |
| `-s`, `--session ID` | Open a session by id, live or from history. |
| `-w`, `--watch` | Split view of all live background sessions. Creates no session. |
| `--agent NAME` | Agent for the new session. |
| `--model provider/model[#variant]` | Model for the new session. Not remembered for later sessions. |
| `-p`, `--prompt TEXT` | Send this prompt right away. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token for `--server`. |
| `--mock` | Run against a built-in fake backend (for development). |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

Without `--server`, `ref` looks for a running backend in the instance files (see below) and connects to it. If none is running, it starts `ref-server` from the same directory as `ref` (or from `PATH`) with a random free port and `--idle-timeout 0`, so the backend exits when the last `ref` disconnects. Turns that are running stop with it; start `ref-server` yourself (below) to keep sessions running without a window.

## `ref-web`

The web UI server. See [Web UI](web.md) for what it does.

```
ref-web [DIR] [options]
```

| Option | Meaning |
|---|---|
| `DIR` | Directory for new sessions (default: the current directory). |
| `--port N` | Port to listen on (1–65535, default `7480`). |
| `--host ADDR` | Address to listen on (default `127.0.0.1`). Any other address opens `ref-web` to the network, without TLS; a warning is printed. |
| `--no-open` | Do not open the browser. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token for `--server`. Needs `--server`. |
| `--mock` | Use a private fake backend (for development). Cannot be combined with `--server`. |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

Without `--server`, `ref-web` finds the backend the same way `ref` does. The login token for browsers is kept in `<state dir>/web/login-token`.

## `ref-server`

The backend. You normally do not start it yourself, but you can, for example to keep sessions running without a window or to connect from another machine.

```
ref-server [options]
```

| Option | Meaning |
|---|---|
| `--port N` | Port. Absent or `0` picks a random free port in 10000–65535. |
| `--host ADDR` | Address to bind. Default `127.0.0.1`, or `0.0.0.0` when a non-loopback host is allowed. |
| `--allow-host H` | Hosts or IPs allowed to connect. Repeatable and comma-separated; `*` allows any. Default: `127.0.0.1`, `::1`, `localhost`. |
| `--idle-timeout S` | Seconds to keep running after the last client disconnects. `0` exits at once, `-1` (the default) never. Only counts after the first connection. |
| `--token T` | Secret that clients must send. Default: random, stored in the instance file. |
| `--log FILE` | Log file. Default: `~/.local/state/ref/logs/server-<pid>.log`. |
| `--status [--json]` | List running backends with their port, address, directory and settings. Exit code 1 when none is running. |
| `--stop [PID]` | Stop all running backends, or the one with this PID. |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

## Running the backend on another machine

```bash
# On the server
ref-server --port 7777 --allow-host 192.168.1.20 --token "$(openssl rand -hex 16)" --idle-timeout -1

# On your machine
ref --server ws://server:7777 --token <the same token>
```

The connection is plain WebSocket without TLS, and anyone who has the token controls the backend, which can run commands on that machine. Only allow hosts you trust, or connect through an SSH tunnel and keep the default loopback-only setting:

```bash
ssh -L 7777:127.0.0.1:7777 server
ref --server ws://127.0.0.1:7777 --token <token>
```

## Files and directories

| Path | Contents |
|---|---|
| `~/.config/ref/`, `~/.config/opencode/` | Configuration. See [Configuration](configuration.md#where-configuration-is-read-from). |
| `~/.local/share/ref/sessions/` | Saved sessions. |
| `~/.local/state/ref/instances/<pid>.json` | One file per running backend: port, token and settings. Readable only by you (mode 600). |
| `~/.local/state/ref/prompt-history.jsonl` | Prompt history (500 entries), shared by `ref` and `ref-gui`. |
| `~/.local/state/ref/gui.json` | Settings of `ref-gui` ([Settings](gui.md#settings)). |
| `~/.local/state/ref/web/login-token` | Login token of `ref-web`. Readable only by you (mode 600). Delete it while `ref-web` is stopped to get a new one. |
| `~/.local/state/ref/logs/` | Backend logs, including configuration warnings. |

On Windows, `~` is `%USERPROFILE%`.

## Environment variables

| Variable | Meaning |
|---|---|
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`, `OPENROUTER_API_KEY` | Add the corresponding built-in provider (see [Quick start](configuration.md#quick-start)). |
| `OPENCODE_CONFIG`, `REF_CONFIG` | Extra configuration file to load. |
| `OPENCODE_CONFIG_DIR` | Use this directory instead of `~/.config/opencode`. |
| `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME` | Replace `~/.config`, `~/.local/share` and `~/.local/state`. |
| `REF_DATA_DIR`, `REF_STATE_DIR` | Replace the data and state directories directly (`…/share/ref` and `…/state/ref`). |
| `REF_LOG` | Backend log filter, in `tracing` syntax (default `info`; for example `debug` or `ref_llm=trace`). |
| `REF_DUMP_REQUESTS=DIR` | Write every LLM request body to `DIR` (headers, which hold the API key, are never written). |
| `REF_TUI_LOG=FILE` | Write the TUI's input debug log to `FILE`. |
| `GITHUB_TOKEN`, `GH_TOKEN` | Used by `webfetch` for GitHub API requests. |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | Proxy for outgoing HTTP: LLM APIs, `webfetch` and remote MCP servers. |
| `VISUAL`, `EDITOR` | Editor for `ctrl+x e` (default `vi`, or `notepad` on Windows). |
