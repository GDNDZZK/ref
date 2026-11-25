**English** · [简体中文](cli.zh-CN.md)

# Command line

- [`ref`](#ref)
- [`ref-gui`](#ref-gui)
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
| `PATH` | Default directory for the new session (default: the current directory). |
| `-c`, `--continue` | Continue the most recent session for this directory (warns when none exists). |
| `-s`, `--session ID` | Open a session by id, live or from history. |
| `-w`, `--watch` | Split view of all live background sessions. Creates no session. |
| `--agent NAME` | Pre-selected agent for the new session. |
| `--model provider/model[#variant]` | Pre-selected model for the new session. Not remembered for later sessions. |
| `-p`, `--prompt TEXT` | Create a session and send this prompt right away. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token for `--server`. |
| `--mock` | Run against a built-in fake backend (for development). |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

Without `-c`, `-s`, `-w`, or `-p`, `ref` starts on an empty welcome screen without creating a session; sending the first prompt (or running `/new`) creates one with the pre-selected directory, agent, model, and variant. Passing `-c` when the directory has no saved session warns instead of creating one.

Without `--server` (and when no saved default backend is set in `tui.json`), `ref` looks for a running backend in the instance files (see below) and connects to it. If none is running, it starts `ref-server` lazily from the same directory as `ref` (or from `PATH`) with a random free port and `--idle-timeout 0`, so the backend exits when the last `ref` disconnects. Passing `--server` or saving a default backend skips local `ref-server` startup. Turns that are running stop with a locally spawned server; start `ref-server` yourself (below) to keep sessions running without a window.

## `ref-gui`

The desktop GUI. See [Desktop GUI](gui.md) for details.

```
ref-gui [DIR] [options]
```

| Option | Meaning |
|---|---|
| `DIR` | Default directory for new sessions (default: the current directory). |
| `-c`, `--continue` | Continue the most recent session for this directory. |
| `-s`, `--session ID` | Open a session by id, live or from history. |
| `-w`, `--watch` | Start in the split view of all live background sessions. |
| `--agent NAME` | Default agent for new sessions. |
| `--model provider/model[#variant]` | Default model for new sessions. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token for `--server`. Needs `--server`. |
| `--mock` | Use a private fake backend (for development). Cannot be combined with `--server`. |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

Without `-c` or `-s`, `ref-gui` opens in the empty workspace state (*No session open*) without auto-creating a session; `DIR`, `--agent` and `--model` set defaults for sessions you create from the window (and creating a session in another folder updates the default directory). Local `ref-server` startup is skipped when `--server` or a saved default `backendUrl` in `gui.json` is configured. The hidden `--smoke` flag prints `ready` when started without a session flag, or `attached <id>` with `-c` / `-s`, and exits with code 0.

## `ref-web`

The web UI server. See [Web UI](web.md) for what it does.

```
ref-web [DIR] [options]
```

| Option | Meaning |
|---|---|
| `DIR` | Default directory for new sessions (default: the current directory). Opening the page without `?session=<id>` starts in the empty workspace state without creating a session. |
| `--port N` | Port to listen on (1–65535). Without it: a random free port in 10000–65535; the port of the last run is tried first, so the address stays the same. |
| `--host ADDR` | Address to listen on (default `127.0.0.1`). Any other address opens `ref-web` to the network and enables HTTPS with a self-signed certificate by default; pass `--no-tls` for plain HTTP. |
| `--no-tls` | Serve plain `http://` even on a non-loopback `--host` (for example behind a reverse proxy). On a loopback `--host` this is a silent no-op. |
| `--key KEY` | The key browsers sign in with, at least 16 characters; not written to disk. Also read from the environment variable `REF_WEB_KEY`, which keeps it out of the process list. Default: the login token. See [Signing in](web.md#signing-in). |
| `--no-open` | Do not open the browser. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token of the backend given with `--server` (not the browser's key, see `--key`). Needs `--server`. |
| `--mock` | Use a private fake backend (for development). Cannot be combined with `--server`. |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

Without `--server`, `ref-web` resolves the backend lazily when a browser connects: if the browser sends an explicit saved backend (`ref.backend.v1`) over the sealed control channel, `ref-web` connects directly to it without starting a local `ref-server`; otherwise it finds or starts one the same way `ref` does. Without `--key`, the key for browsers is the login token kept in `<state dir>/web/login-token`.

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

`ref-server --stop` and `--status` work across protocol versions. If a UI client and `ref-server` speak different protocol versions, `ref-server` refuses the connection with a message telling you whether to update the client or run `ref-server --stop` and start again.
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
| `~/.config/ref/`, `~/.config/opencode/` | Configuration, including `ref.json` and numbered backup profiles `ref.json.<n>.bak` (mode 700 / 600 on Unix; leftover `*.tmp.json` files are cleaned up on `ref-server` start). See [Configuration](configuration.md#where-configuration-is-read-from). |
| `~/.local/share/ref/sessions/` | Saved sessions. |
| `~/.local/state/ref/instances/<pid>.json` | One file per running backend: port, token and settings. Readable only by you (mode 600). |
| `~/.local/state/ref/prompt-history.jsonl` | Prompt history (500 entries), shared by `ref` and `ref-gui`. |
| `~/.local/state/ref/tui.json` | Preferences of `ref` (saved default `backendUrl` and `backendToken`). Readable only by you (mode 600 on Unix). |
| `~/.local/state/ref/gui.json` | Settings of `ref-gui` ([Settings](gui.md#settings)). |
| `~/.local/state/ref/web/login-token` | Login token of `ref-web`, the default key. Readable only by you (mode 600). Delete it while `ref-web` is stopped to get a new one. |
| `~/.local/state/ref/web/salt` | Salt for the key (16 random bytes as hex, not secret; mode 600). Browsers keep a value derived from the key and the salt; a new salt makes them ask for the key again. |
| `~/.local/state/ref/web/port` | The port `ref-web` used last, tried first when no `--port` is given. |
| `~/.local/state/ref/web/instances/<pid>.json` | One file per running `ref-web`: port, host, tls, version, start time. No secret; mode 600. |
| `~/.local/state/ref/web/tls/` | Self-signed TLS certificate (`cert.pem`, `meta.json`) and private key (`key.pem`, mode 600; directory mode 700), created when a non-loopback `--host` serves HTTPS. |
| `~/.local/state/ref/logs/` | Backend logs, including configuration warnings. |

On Windows, `~` is `%USERPROFILE%`. The modes (600, and 700 for directories) apply on Unix only; on Windows the files rely on the access rights of the user profile.

## Environment variables

| Variable | Meaning |
|---|---|
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`, `OPENROUTER_API_KEY` | Add the corresponding built-in provider (see [Quick start](configuration.md#quick-start)). |
| `OPENCODE_CONFIG`, `REF_CONFIG` | Extra configuration file to load. |
| `OPENCODE_CONFIG_DIR` | Use this directory instead of `~/.config/opencode`. |
| `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME` | Replace `~/.config`, `~/.local/share` and `~/.local/state`. |
| `REF_DATA_DIR`, `REF_STATE_DIR` | Replace the data and state directories directly (`…/share/ref` and `…/state/ref`). |
| `REF_WEB_KEY` | `--key` of `ref-web`: the key browsers sign in with (at least 16 characters). |
| `REF_LOG` | Backend log filter, in `tracing` syntax (default `info`; for example `debug` or `ref_llm=trace`). |
| `REF_DUMP_REQUESTS=DIR` | Write every LLM request body to `DIR` (headers, which hold the API key, are never written). |
| `REF_TUI_LOG=FILE` | Write the TUI's input debug log to `FILE`. |
| `GITHUB_TOKEN`, `GH_TOKEN` | Used by `webfetch` for GitHub API requests. |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | Proxy for outgoing HTTP: LLM APIs, `webfetch` and remote MCP servers. |
| `VISUAL`, `EDITOR` | Editor for `ctrl+x e` (default `vi`, or `notepad` on Windows). |
