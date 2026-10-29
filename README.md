# ref

A terminal coding agent written in Rust. The backend (`ref-server`) does all the real work: sessions, the agent loop, LLM calls, tools, MCP, permissions and cross-session messaging. The TUI (`ref`) only renders and sends requests. The two talk over WebSocket.

```
cargo build --release
./target/release/ref            # new session in the current directory
```

`ref` looks for a running backend and connects to it. If none is running, it starts one on a random free port ≥ 10000 with `--idle-timeout 0`, so the backend exits when the last frontend leaves.

## Platforms

Linux, macOS and Windows (x86_64 and arm64).

- The `bash` tool needs bash. On Windows install [Git for Windows](https://git-scm.com/download/win); ref finds its Git Bash through `git.exe` in `PATH` or the default install locations. The WSL `bash.exe` in `System32` is skipped because it runs commands inside the Linux VM with different paths.
- On Windows, a stopped or timed-out command is killed with its whole process tree right away (`taskkill /T /F`). Unix gets SIGTERM first and SIGKILL after 2 s.
- The state and data directories below are under the user's home directory on every platform (`%USERPROFILE%` on Windows).
- `$EDITOR` defaults to `vi`, or `notepad` on Windows.
- MCP stdio commands such as `npx` also resolve to `npx.cmd` on Windows (through `PATHEXT`).

## Binaries

### `ref-server`

| Flag | Meaning |
|---|---|
| `--port N` | Port. Absent or `0` picks a random free port in 10000–65535. |
| `--host ADDR` | Bind address. Default `127.0.0.1`, or `0.0.0.0` when a non-loopback host is allowed. |
| `--allow-host H` | Hosts/IPs allowed to connect (repeatable, comma separated, `*` = any). Default: loopback only. |
| `--idle-timeout S` | Seconds to stay alive after the last frontend disconnects. `0` = exit immediately, `-1` = never. Only counts after the first connection. |
| `--token T` | Shared secret clients must present (default: random, stored in the instance file). |
| `--status [--json]` | List running instances with their configuration. Exit code 1 if none. |
| `--stop [PID]` | Stop all instances, or the one with PID. |

Instance files are written to `~/.local/state/ref/instances/<pid>.json` (mode 600). Sessions are stored in `~/.local/share/ref/sessions/`. Logs go to `~/.local/state/ref/logs/`. `REF_STATE_DIR` and `REF_DATA_DIR` override these locations.

### `ref`

| Flag | Meaning |
|---|---|
| `[PATH]` | Working directory for the new session. |
| `-s ID` | Attach to an existing session (live or from history). |
| `-c` | Continue the most recent session for this directory. |
| `-w`, `--watch` | Split view of all live **background** sessions. Creates no session. |
| `--agent NAME`, `--model provider/model[#variant]` | Agent and model for the new session (not remembered). |
| `-p TEXT` | Send an initial prompt. |
| `--server ws://host:port --token T` | Connect to an explicit backend (no auto-spawn). |

A session is **foreground** while a frontend is directly attached to it, and **background** otherwise. Watching a session in split view does not make it foreground.

## TUI

- The sidebar's Live sessions list shows, right-aligned per session, its todo count (`2/5`; `✓3/3` in green when all are done; cancelled items are not counted) and, while busy, the working time.
- Every message shows its time: your prompts, each assistant reply (start → end, duration, tokens), and each tool call (start, duration).
- Collapsed tool calls show the tool name and the model's `purpose`. Click one to see the raw input and output.
- Collapsed thinking shows, on a single line, the latest characters of the reasoning, updated live.
- Selecting text with the mouse never copies it. Copy with `ctrl+c`, `y` or right-click. The `⧉ copy` button under every message copies its raw markdown.
- `/` commands, `@` file mentions, `!` shell mode, `ctrl+v` to paste an image (pasting an image path works too), `shift+enter` / `ctrl+j` for a newline, `esc esc` to interrupt.
- Prompts sent while the session is busy are queued and shown under the transcript until the next safe point (after the current tool step). Click `✕ remove` on a queued prompt to withdraw it before it is sent: its text and images go back into the input box (appended after a blank line if you were typing). If it was delivered in the meantime, a toast says so. Messages queued by other sessions (and reminders) are listed too, labelled with their sender, but cannot be removed. Every frontend attached to the session sees the queue update live.
- Undo (`ctrl+x u`, `/undo`, or `↶ undo` under a prompt to undo from that prompt) removes the prompt and everything after it and puts the prompt's text and images back into the input box, appended after a blank line if you were typing. Files are not restored.
- While a session works, the status line and its row under Live sessions in the sidebar show how long the current turn has been running (taken from the backend, so it survives switching sessions).
- Leader key `ctrl+x`: `n` new session, `l` sessions (live fg/bg plus history), `m` models, `a` agents, `t` thinking, `d` tool details, `c` compact, `u` undo, `e` $EDITOR, `x` export, `w` split view, `h` help, `q` quit. `ctrl+p` opens the command palette. `tab` cycles agents, `ctrl+t` cycles the model variant.
- Split view (`ref -w` or `ctrl+x w`): sessions are tiled automatically. `tab` or `alt+arrows` move focus, typing and `enter` message the focused session, `esc esc` aborts it, `ctrl+o` zooms in. `/close` and `/cd` typed in a tile act on that tile's session.

Slash commands: `/new /sessions /models /agents /variant /compact /undo /rename /cd /close /export /thinking /details /watch /editor /help /exit`. Commands from your config are listed too.

- `/cd DIR` (alias `/cwd`) moves the session to another directory: absolute, relative to the current one, or `~/…`. Project config and instructions are reloaded, and a reminder telling the model about the change is added to the conversation (no LLM turn). Not allowed while the session is busy. New sessions from this frontend then start there too. Without an argument it shows the current directory.
- `/close` (or `ctrl+x k` twice) closes the current session and switches to the oldest remaining live session. When none is left, `ref` exits; a backend that `ref` started itself then exits too.

Cross-session commands run in the backend without an LLM turn:

- `/peers` (alias `/list-agents`): other live sessions with status, agent, fg/bg, directory and queued messages.
- `/peers-name [name]`: show or set this session's peer name (any Unicode, unique among live sessions).
- `/peers-inbox`: messages received from other sessions, including ones still queued.
- `/peers-outbox`: messages this session sent and whether each is queued, delivered or read.

## Configuration

ref reads opencode's configuration, so an existing `opencode.json` works as is (providers, models, variants, agents, permission, mcp, instructions, command). Files are merged in this order, later ones winning:

1. `~/.config/opencode/opencode.json[c]`, then `~/.config/ref/ref.json[c]`
2. `$OPENCODE_CONFIG`, `$REF_CONFIG`
3. From the git root down to the cwd: `opencode.json[c]`, `.opencode/opencode.json[c]`, `ref.json[c]`, `.ref/ref.json[c]`

JSONC comments and trailing commas are accepted. `{env:VAR}` and `{file:path}` are substituted in string values. Agents and commands can also be markdown files under `agent(s)/` and `command(s)/` in `~/.config/opencode`, `~/.config/ref` and `.opencode/` / `.ref/` directories.

Provider API selection follows opencode's `npm` field: `@ai-sdk/anthropic` uses Anthropic Messages, `@ai-sdk/openai` uses OpenAI Responses, and anything else uses OpenAI Chat Completions. The `api` field overrides this (`"chat"`, `"responses"`, `"anthropic"`).

ref-specific keys:

```jsonc
{
  // Default model when the agent has none.
  "model": "anthropic/claude-sonnet-4-5",

  // Permission per tool: "allow" | "ask" | "deny", or an object of patterns (last match wins).
  "permission": {
    "bash": { "*": "ask", "git status*": "allow", "rm *": "deny" },
    "delete": "ask"
  },

  // Seconds to wait before running a tool. During the countdown esc cancels it.
  "delay": { "bash": 2, "delete": 5 },

  // Context management. Values ≤ 1 are fractions of the window; larger values are tokens.
  "compaction": { "auto": true, "recommend": 0.6, "threshold": 0.9 },

  // Retry backoff cap in seconds. Retryable errors are retried forever.
  "retry": { "max_delay": 600 },

  "agent": {
    "build": { "can_spawn": true },
    "reviewer": {
      "description": "Reviews diffs and reports problems.",  // shown to agents that can create sessions
      "prompt": "{file:./prompts/reviewer.md}",
      "model": "dgn/claude-opus-5.5",
      "variant": "high",
      "creatable": true,          // may be created by other sessions (default true)
      "can_spawn": false,         // may create/close other sessions (default false; true for build)
      "permission": { "edit": "deny" },   // overrides the global setting
      "delay": { "bash": 0 }
    }
  },

  "provider": {
    "mymodels": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://example.com/v1", "apiKey": "{env:MY_KEY}" },
      "models": {
        "some-model": {
          "limit": { "context": 200000, "output": 32000, "compact": 120000 },
          "attachment": true,
          "reasoning": true,
          "variants": { "high": { "reasoningEffort": "high" } }
        }
      }
    }
  }
}
```

Built-in agents: `build` (default, can create sessions), `plan` (read-only), `general` and `explore` (read-only). A new session always starts with its agent's default model. Switching the model affects only that session.

Context files: `~/.config/opencode/AGENTS.md`, `~/.config/ref/AGENTS.md`, then `AGENTS.md` (or `CLAUDE.md` if there is none) in every directory from the git root down to the cwd, plus `instructions` globs. Skills: `SKILL.md` files under `skill(s)/` in config directories, `.opencode/`, `.claude/skills`, `.agents/skills` and `~/.claude/skills`. They are listed in the system prompt and the model reads them with `read`.

## Tools

Built-in tools: `read`, `edit`, `write`, `delete`, `grep`, `find` and `bash`. Every tool, including MCP tools, takes a `purpose` argument that the TUI displays.

File tools use hashline anchors, modeled on oh-my-pi. `read` and `grep` print a `[path#TAG]` header (a hash of the whole file) followed by `N:content` rows. `edit` addresses original line numbers under that tag and applies all its edits at once. Stale tags are remapped through a diff when possible and rejected with the current content otherwise.

Harness tools:

- `context_window`: tokens used, the maximum, and the recommended and automatic compaction points, for this session or another one.
- `compact_context`: replaces the history with a summary the model writes.

Cross-session tools (in-process, so delivery is reliable; session names may be any Unicode):

- `list_sessions`, `send_message`, `message_status`, `read_session` (paginated).
- `create_session` and `close_session`, only for agents with `can_spawn`. Created sessions run in the background. With `must_report`, a session that stops without sending a message gets a reminder. `close_session` refuses foreground sessions.

LLM failures retry with exponential backoff (2s doubling, capped at `retry.max_delay`, default 600s), forever for retryable errors. Tool errors are returned to the model and never end the loop.

## Development

```
cargo test                      # all crates
python3 scripts/e2e.py dgn/claude-sonnet-5.5 "prompt" [cwd]   # end-to-end against a real model
REF_DUMP_REQUESTS=/tmp/dump ref-server ...                    # dump LLM request bodies (no headers)
ref --mock                      # TUI against a fake backend
```

Crates: `proto` (wire protocol), `config`, `llm` (Chat / Responses / Anthropic streaming), `tools`, `mcp` (stdio + streamable HTTP), `sys` (OS layer), `server`, `tui`.

Frontends get history page by page: `Attach` returns the newest messages (100, fewer if they exceed about 1 MiB) and `Messages` returns the page before a given message; the TUI loads older pages as you scroll up. Images reach frontends as stubs (mime, size and an `id`, no bytes); `GetImage` returns the bytes for an id.
