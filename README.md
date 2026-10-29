# hernees

> **简介：** hernees 是一个用 Rust 编写的终端编程助手（coding agent）。后端 `hernees-server`
> 负责会话、agent 循环、LLM 调用、工具、MCP 与持久化；前端 `hernees` 是 TUI，通过 WebSocket
> 连接后端。直接读取 opencode 的配置文件（provider / model / agent / mcp / permission 等），
> 并增加了工具延迟执行、压缩阈值、重试上限、多会话协作等扩展。

hernees is a coding-agent harness: a backend that runs agent sessions (LLM streaming, tools,
MCP servers, persistence) and a terminal UI that talks to it over a WebSocket protocol.
Several frontends can share one backend; sessions keep running when no frontend shows them
and can message, spawn and inspect each other.

## Build and install

```sh
cargo build --release
# binaries: target/release/hernees  (TUI)
#           target/release/hernees-server  (backend)
install -m755 target/release/hernees target/release/hernees-server ~/.local/bin/
```

`hernees` looks for `hernees-server` next to its own binary, then in `PATH`; `$HERNEES_SERVER`
overrides both.

## Usage

```sh
hernees                      # new session in the current directory
hernees "fix the failing test"   # new session, send this prompt right away
hernees -c                   # continue the most recent session of this directory
hernees -s ses_01j…          # resume a session by id
hernees -m openai/gpt-5#high --agent plan
hernees -b                   # background monitor
hernees-server --status      # list running backends
```

### `hernees` (TUI)

| Flag | Meaning |
|---|---|
| `[PROMPT]` | Prompt to send as soon as the session is ready |
| `-s, --session <ID>` | Resume (attach to) a session by id |
| `-c, --continue` | Continue the most recently updated session of the current directory |
| `--agent <NAME>` | Agent for a new session (default: `default_agent`, else `build`) |
| `-m, --model <MODEL>` | `provider/model[#variant]` for a new session (default: the agent's model, else `model`) |
| `--server <URL>` | Use this backend (`ws://host:port`) instead of discovering or starting one |
| `-b, --background` | Background monitor (see below) |

Without `--server`, the TUI connects to a live backend from the instance registry
(`$XDG_STATE_HOME/hernees/instances/<pid>.json`, default `~/.local/state/hernees/instances`). If
none answers, it starts `hernees-server --idle-timeout 0` (log in
`~/.local/state/hernees/logs/`). Such a backend exits when its last frontend disconnects. If
a backend is shutting down while a frontend connects, the connection is refused with HTTP 503
(or closes during the handshake) and the frontend starts another backend. When a connection
drops, the TUI reconnects and re-attaches; sessions are persisted, so nothing is lost.

**Background monitor (`-b`).** Shows every open *background* session (one that no frontend
has in the foreground) as a tiled pane, without creating a session. Panes appear and
disappear as sessions start, finish or get attached elsewhere. You can type into the focused
pane, approve permissions and interrupt it. Background sessions that wait for permission
also raise a toast in every frontend.

### `hernees-server` (backend)

| Flag | Meaning |
|---|---|
| `--port <PORT>` | Port (default: random free port in 10000–60000) |
| `--host <HOST>` | Bind address (default `127.0.0.1`) |
| `--allow-host <HOST>` | Allowed client address, repeatable: IP, host name (resolved at startup), `localhost`, or `*` for any. Default `127.0.0.1` and `::1`. Others get HTTP 403 |
| `--idle-timeout <SECONDS>` | `0` = exit as soon as the last frontend disconnects; `-1` = never (default); `N` = after N seconds without a frontend. The timer starts only after the first frontend connected (with `0`, the server also exits if no frontend connects within 120 s). `--status` probes don't count as frontends |
| `--status` (alias `--list`) | List running instances (pid, URL, idle timeout, open/busy sessions, connected frontends); exit code 0 if any is running |
| `--json` | With `--status`: JSON output |

Shutdown (signal or idle timeout) removes the instance file first, then flushes all sessions
(in-flight turns are saved as aborted), then closes the listener. With an idle timeout, a
frontend that connects before the shutdown is committed cancels it.

Logs go to stderr; filter with `HERNEES_LOG` (e.g. `HERNEES_LOG=debug`). At debug level,
requests that take longer than 1 s are logged with their duration.

Environment: `HERNEES_DATA_DIR` (sessions, default `~/.local/share/hernees`),
`HERNEES_STATE_DIR` (instances, logs, prompt history, default `~/.local/state/hernees`),
`HERNEES_CONFIG_DIR` / `OPENCODE_CONFIG_DIR`, `HERNEES_CONFIG` / `OPENCODE_CONFIG` (extra config
file), `HERNEES_MODELS_PATH` / `OPENCODE_MODELS_PATH` (models.dev catalog).

**Proxies.** `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` (http(s) proxies only) are honored
for LLM providers and remote MCP servers. Loopback, `localhost` and private / link-local
addresses always connect directly. `NO_PROXY` accepts `*`, IPs, CIDRs, domains (`example.com`
and `.example.com` include subdomains) and shell wildcards (`127.*`, `*.local`, `*corp.com`).
An HTTP error with an empty body is reported as `HTTP <status> (empty body; possibly from proxy
<url>)` when the request went through a proxy.

## Configuration

hernees reads opencode's configuration, so an existing opencode setup works as is. Files are
deep-merged, later ones winning:

1. `~/.config/opencode/opencode.json`, `opencode.jsonc`, plus `agent(s)/`, `mode(s)/`, `command(s)/` markdown in that directory
2. `~/.config/hernees/hernees.json`, `hernees.jsonc` (same subdirectories)
3. `$OPENCODE_CONFIG`, `$HERNEES_CONFIG`
4. For each directory from the git root (or `/`) down to the working directory: `opencode.json(c)`,
   `.opencode/opencode.json(c)` and `.opencode/{agent,agents,mode,modes,command,commands}/*.md`,
   `hernees.json(c)`, `.hernees/hernees.json(c)` and `.hernees/…/*.md`

JSONC (comments, trailing commas) and `{env:VAR}` / `{file:path}` substitution are supported.
Model metadata missing from the config (context/output limits, reasoning, image input) is
filled from the models.dev catalog at `~/.cache/opencode/models.json`. The config is reloaded
automatically (cached for 3 s).

Supported opencode keys:

- `provider`: `npm` selects the wire protocol (`@ai-sdk/anthropic` → Anthropic Messages,
  `@ai-sdk/openai` / `@ai-sdk/azure` → OpenAI Responses, anything else → Chat Completions),
  `options` (`baseURL`, `apiKey`, `headers`, `body`/`extraBody`, `chunkTimeout`, other keys passed
  as request options), `env`, `models` (`id`, `name`, `limit.{context,input,output}`, `reasoning`,
  `attachment`, `modalities`, `interleaved`, `options`, `headers`, `variants`, `provider.{npm,api}`,
  `cost`), `whitelist`, `blacklist`; `enabled_providers`, `disabled_providers`
- `model`, `small_model` (used for session titles), `default_agent`
- `agent` (and legacy `mode`): `description`, `prompt`, `model`, `variant`, `mode`, `disable`,
  `hidden`, `color`, `permission`, `tools`, `options`; markdown agents use frontmatter + body
- `mcp`: `{"type": "local", "command": [...], "environment", "cwd"}` or
  `{"type": "remote", "url", "headers"}`, both with `enabled` and `timeout` (ms, default 30000)
- `permission`: `"allow"` or `{tool: action | {pattern: action}}` with actions
  `allow` / `ask` / `deny`; the last matching pattern wins, agent rules override global ones.
  The pattern is matched against the bash command or the file path. `write`/`delete` also match
  `edit` rules, `find` also matches `glob`/`list`. Deprecated `tools: {name: false}` works too
- `instructions`: extra instruction files / globs, relative to the project root. `AGENTS.md`
  (else `CLAUDE.md`) is also read from each directory between the git root and the working
  directory, plus one global file: the first of `~/.config/hernees/AGENTS.md`,
  `~/.config/opencode/AGENTS.md`, `~/.claude/CLAUDE.md`
- `command`: `{template, description, agent, model}`; templates use `$ARGUMENTS`, `$1`…`$N`
- Skills: `SKILL.md` files under `~/.claude/skills`, `~/.agents/skills`,
  `~/.config/{opencode,hernees}/skill(s)`, and project `.claude/skills`, `.agents/skills`,
  `.opencode/skill(s)`, `.hernees/skill(s)`. They are listed to the model, loaded with the
  `skill` tool, and runnable as `/name`

Model references are `provider/model[#variant]`; a variant overlays its options (e.g.
`reasoningEffort`) on the model's options. `ctrl+t` cycles variants.

### hernees extensions

```jsonc
{
  // Seconds to wait before running a tool (glob keys; the last matching key wins).
  // The TUI shows a countdown; esc esc cancels the call.
  "delay": { "bash": 5, "mcp_*": 2 },
  // Recommended compaction threshold as a fraction of the context window (default 0.8);
  // `auto` (default true) compacts with an LLM summary when the context is about to overflow.
  "compaction": { "ratio": 0.75, "auto": true },
  // Retryable errors are retried forever with exponential backoff:
  // initial_delay seconds (default 2), capped at max_delay seconds (default 600).
  "retry": { "initial_delay": 2, "max_delay": 120 },
  "agent": {
    "reviewer": {
      "description": "Reviews diffs",
      "prompt": "You review code changes…",
      "spawnable": true,        // other sessions may start it with create_session
      "manage_sessions": false, // may itself use create_session / close_session
      "delay": { "*": 0 }       // per-agent delays override the global ones
    }
  },
  "provider": {
    "openai": {
      "models": {
        // Recommended compaction threshold in tokens for this model (overrides the ratio).
        "gpt-5": { "limit": { "context": 400000, "output": 128000, "compact": 250000 } }
      }
    }
  }
}
```

`spawnable` defaults to true for opencode `mode: subagent|all` agents that have a
description. The built-in `build` agent is spawnable and may manage sessions.

## Tools

Every tool takes a required `purpose` string, shown to the user.

| Tool | |
|---|---|
| `read` | Read a file (hashline output: `[PATH#TAG]` header and `N:text` rows; paging with `offset`/`limit`) or list a directory; images inline |
| `edit` | Patch files with hashline ops: `PUT N.=M:` replace, `PUT <N:` / `PUT >N:` insert, `CUT` delete (with registers), `REM` delete file, `MV` rename; anchored to the tag from the last `read`/`grep`/`edit` |
| `write` | Create or overwrite a file |
| `delete` | Delete a file, or a directory with `recursive: true` |
| `grep` | Regex search (gitignore respected), hashline output usable by `edit` |
| `find` | Files matching a glob, newest first |
| `bash` | Run a command with `bash -c` (default timeout 120 s, max 3600; long output is truncated to the tail, full log kept) |
| `context_info` | Context window usage and the recommended compaction point |
| `compact_context` | Replace the older context with a summary written by the model |
| `list_sessions` | Open sessions (optionally closed ones too) |
| `send_message` | Message another session; it wakes up if idle |
| `session_messages` | Read another session's transcript |
| `session_context` | Another session's context usage |
| `create_session` | Start a session with a spawnable agent and a task (`must_report` asks it to report back); agents with `manage_sessions` only |
| `close_session` | Close a session; agents with `manage_sessions` only |
| `skill` | Load a skill (present when skills exist) |

MCP tools are added as `<server>_<tool>`. MCP servers connect in the background when a
session opens. Only a session's first step waits for servers that are still connecting, for
at most 10 s, so a slow server never blocks the UI.

## Key bindings

| Keys | |
|---|---|
| `enter` | Send prompt |
| `shift+enter`, `alt+enter`, `ctrl+j` | Newline |
| `esc esc` | Interrupt the running session |
| `ctrl+c` | Clear input; twice to exit (`ctrl+d` on empty input exits) |
| `ctrl+p` | Command palette |
| `tab` / `shift+tab` | Cycle agents |
| `ctrl+t` | Cycle model variant |
| `↑` / `↓` | Prompt history (single-line input) |
| `pgup` / `pgdn` / wheel | Scroll; `ctrl+g` / `ctrl+alt+g` top / bottom |
| `alt+↑` / `alt+↓` | Focus previous / next tool or thinking block; `enter` / `space` expands it |
| drag | Select text; `ctrl+y`, `ctrl+shift+c` or `y` copies |
| `F2`, `ctrl+x s` | Native terminal selection (mouse capture off) |
| `ctrl+v` | Paste image (or text) from the clipboard |
| `ctrl+x n` / `l` / `o` / `k` | New session / sessions / open (attach to background) / close a background session |
| `ctrl+x m` / `a` | Models / agents |
| `ctrl+x c` / `u` / `r` | Compact / undo / rename |
| `ctrl+x x` / `y` / `e` | Export / copy last response / compose in `$EDITOR` |
| `ctrl+x t` / `d` / `b` | Toggle thinking / tool details / sidebar |
| `ctrl+x h` or `?` | Help |
| `ctrl+x q` | Exit |
| Monitor: `alt+arrows`, `ctrl+x arrows`, `ctrl+x 1`–`9` | Focus pane |
| Monitor: `alt+z`, `ctrl+x z` | Zoom pane |
| Monitor: `ctrl+x [` / `]` | Previous / next page |

Permission prompts: `←`/`→` select, `enter` confirm, `esc` reject.

## Slash commands

`/new` (`/clear`), `/sessions` (`/resume`, `/history`), `/open` (`/active`), `/models` (`/model`),
`/agents` (`/agent`), `/compact` (`/summarize`), `/undo`, `/rename`, `/export` (`/export copy` for
the clipboard), `/copy`, `/editor`, `/thinking`, `/details`, `/close`, `/sidebar`, `/select`
(`/mouse`), `/help`, `/exit` (`/quit`, `/q`). Custom commands from `command` and skills are
also available as `/name [args]`.

## Architecture

| Crate | |
|---|---|
| `crates/proto` (`hn-proto`) | WebSocket protocol types (requests, responses, session events), shared paths |
| `crates/llm` (`hn-llm`) | Streaming clients for Chat Completions, Responses and Anthropic Messages; error classification; proxy selection |
| `crates/tools` (`hn-tools`) | Built-in tools and the hashline edit engine |
| `crates/config` (`hn-config`) | opencode-compatible config loading/merging, model resolution, permissions, skills, instructions |
| `crates/mcp` (`hn-mcp`) | MCP client (stdio, Streamable HTTP with legacy SSE fallback) |
| `crates/server` (`hernees-server`) | Backend: hub of clients and sessions, agent loop, compaction, retries, peer tools, persistence, instance registry |
| `crates/tui` (`hernees`) | Terminal UI (ratatui): rendering, input, dialogs, backend discovery/spawning, reconnects |

Sessions are stored as JSON under `~/.local/share/hernees/sessions/<id>/`. Each session runs
its own task; every change is pushed as an event to attached frontends.
