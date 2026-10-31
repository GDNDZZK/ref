**English** · [简体中文](usage.zh-CN.md)

# Using ref

- [Starting](#starting)
- [Sessions and the backend](#sessions-and-the-backend)
- [The screen](#the-screen)
- [Writing prompts](#writing-prompts)
- [Reading the transcript](#reading-the-transcript)
- [Keybindings](#keybindings)
- [Slash commands](#slash-commands)
- [Split view](#split-view)
- [Working with several sessions](#working-with-several-sessions)
- [Permissions and delays](#permissions-and-delays)

## Starting

```bash
ref                       # new session in the current directory
ref ~/src/project         # new session in another directory
ref -c                    # continue the most recent session for this directory
ref -s ses_01a1…          # open a session by id (live or from history)
ref -w                    # split view of all background sessions
ref --agent plan          # new session with another agent
ref --model openai/gpt-5#high -p "Explain src/main.rs"
```

All flags are listed in [Command line](cli.md#ref).

## Sessions and the backend

ref has two programs. `ref-server` (the backend) runs the sessions: it calls the LLM, runs tools, stores history and handles permissions. `ref` (the TUI) shows sessions and sends your input. They talk over a local WebSocket.

When you start `ref`, it connects to a running backend or starts one. A backend started this way exits when the last `ref` window closes. Other programs that show sessions, such as the browser UI ([Web UI](web.md)), find the same backend through the same instance file, so a session can be open in several of them at once. A backend started by one of them exits after its last client of any kind disconnects. Sessions keep running in the backend while no window shows them, so you can close a window, open another and continue where the session is.

- A session is **foreground** while a `ref` window is attached to it, and **background** otherwise.
- Sessions are saved in `~/.local/share/ref/sessions/`. `ctrl+x l` lists live sessions and history; opening an old one loads it again.
- Several `ref` windows can show the same session at the same time.

## The screen

- **Transcript**: your prompts, the model's replies, reasoning and tool calls.
- **Input box** at the bottom, with the agent, model and variant next to it.
- **Sidebar** (`ctrl+x b` toggles it):
  - **Live sessions**, each with its todo progress (`2/5`, or a green `✓3/3` when all are done; cancelled items are not counted) and, while busy, how long the current turn has been running.
  - **MCP servers** and their status.
- **Status line**: what the session is doing and the turn's running time. The time comes from the backend, so it is correct after switching sessions.

## Writing prompts

| Input | Effect |
|---|---|
| `enter` | Send. |
| `shift+enter`, `ctrl+j` | New line. |
| `/` | Command list (see [Slash commands](#slash-commands)). |
| `@` | Fuzzy file list; the chosen path is inserted. |
| `!command` | Shell mode: run the command in the session directory (10-minute limit). The command and its output are added to the conversation as your message, without an LLM turn. |
| `ctrl+v` | Paste an image from the clipboard. Pasting the path of an image file attaches it too. |
| `ctrl+x e` | Write the prompt in `$VISUAL` / `$EDITOR` (default `vi`, `notepad` on Windows). |
| `↑` / `↓` | Prompt history, when the input box is empty or the cursor is at its start. |
| `esc esc` | Interrupt the session. |

**Queued prompts.** A prompt sent while the session is busy waits under the transcript and is delivered at the next safe point, after the current tool step. Click `✕ remove` to take it back: its text and images return to the input box (after a blank line if you were typing). If it was delivered in the meantime, a toast says so. Messages queued by other sessions and reminders are listed too, labelled with their sender, but cannot be removed. Every window attached to the session sees the queue change live.

**Undo.** `ctrl+x u`, `/undo`, or `↶ undo` under a prompt removes that prompt and everything after it, and puts the prompt's text and images back into the input box. Files changed by tools are **not** restored.

## Reading the transcript

- Every message shows its time: your prompts, each reply (start, end, duration and tokens) and each tool call (start and duration).
- A collapsed tool call shows the tool name and the `purpose` the model gave. Click it to see the input and output. `ctrl+x d` expands all of them.
- Collapsed reasoning shows its latest characters on one line, updated live. `ctrl+x t` expands it.
- While a turn runs and the view follows the end of the conversation, the bottom of the conversation does not move up. If the content gets shorter, for example when a retry notice disappears, a tool's live output is replaced by its result or expanded reasoning folds to one line, the freed rows stay blank. They are released at once when the turn ends. This also applies to each tile in split view. The rule only starts while the view follows the end; scrolling up does not add blank rows.
- Select text with the mouse: drag, double click for a word, triple click for a line. Selecting does not copy. Copy with `ctrl+c` or a right click (`y` also copies a transcript selection). Typing replaces a selection in the input box.
- `⧉ copy` under a message copies its raw markdown. `ctrl+x y` copies the last reply.

## Keybindings

`ctrl+x` is the leader key: press it, then one of these.

| Key | Action |
|---|---|
| `ctrl+x n` | New session |
| `ctrl+x l` | Sessions: live (foreground and background) and history |
| `ctrl+x m` | Model for this session |
| `ctrl+x a` | Agent |
| `ctrl+x c` | Compact the conversation |
| `ctrl+x u` | Undo the last prompt |
| `ctrl+x e` | Write the prompt in `$EDITOR` |
| `ctrl+x x` | Export the transcript as markdown |
| `ctrl+x y` | Copy the last reply |
| `ctrl+x t` | Expand or collapse reasoning |
| `ctrl+x d` | Expand or collapse tool details |
| `ctrl+x b` | Show or hide the sidebar |
| `ctrl+x w` | Split view |
| `ctrl+x k` (twice) | Close this session |
| `ctrl+x h`, `ctrl+x ?` | Help |
| `ctrl+x q` | Quit |

Other keys: `ctrl+p` opens the command palette, `tab` cycles agents, `ctrl+t` cycles the model's variants and `esc esc` interrupts.

## Slash commands

| Command | Action |
|---|---|
| `/new` (`/clear`) | New session |
| `/sessions` (`/resume`, `/continue`) | Switch session |
| `/models`, `/agents` | Pick a model or an agent |
| `/variant [name]` (`/effort`) | Set or cycle the model variant |
| `/compact` (`/summarize`) | Compact the conversation |
| `/undo` | Undo the last prompt |
| `/rename NAME` | Rename the session |
| `/cd [DIR]` (`/cwd`) | Change the session's directory, or show it |
| `/close` | Close this session |
| `/close-session [name or id]` (`/kill`) | Close live sessions |
| `/export [PATH]` | Export the transcript as markdown (default `ref-session-<id>.md` in the session directory) |
| `/copy` | Copy the last reply |
| `/thinking`, `/details`, `/sidebar` | Toggles |
| `/watch` (`/split`) | Split view |
| `/mcp` | MCP servers: status, errors and tools |
| `/editor` | Write the prompt in `$EDITOR` |
| `/peers`, `/peers-name`, `/peers-inbox`, `/peers-outbox` | See [Working with several sessions](#working-with-several-sessions) |
| `/help` | Keybindings and commands |
| `/exit` (`/quit`, `/q`) | Quit |

Your own commands from the configuration are listed too (see [Custom commands](configuration.md#custom-commands)).

**`/cd DIR`** moves the session to another directory: absolute, relative to the current one, or `~/…`. Project configuration and context files are loaded again, and a note telling the model about the change is added to the conversation, without an LLM turn. It is not allowed while the session is busy. New sessions from this window start in the new directory too.

**Switching the agent** (`tab`, `ctrl+x a` or `/agents`) adds a note to the conversation that tells the model about the change, without an LLM turn. Several quick switches share one note, and switching back to the original agent removes it. While the session is busy the note is added right before the next request.

**`/close`** closes the current session and switches to the oldest remaining live session. When none is left, `ref` exits, and a backend that `ref` started exits with it.

## Split view

`ref -w`, `ctrl+x w` or `/watch` shows the live background sessions side by side.

- The session that was in the foreground in this window becomes background and gets a tile. Leaving split view makes it foreground again.
- `tab` or `alt+arrows` move the focus. Typing and `enter` send a message to the focused session, `esc esc` interrupts it and `ctrl+o` zooms in.
- `/close` and `/cd` typed in a tile apply to that tile's session.
- Each tile shows its queued prompts (`✕ remove` takes one back). Clicking a tool call shows its input and output inside the tile; clicking again collapses it.
- The mouse wheel scrolls the tile under the pointer.

## Working with several sessions

Sessions can talk to each other. The model has tools for it (see [Tools](tools.md#cross-session-tools)), and you have commands that run in the backend without an LLM turn:

- `/peers` (`/list-agents`): other live sessions with their status, agent, foreground or background, directory and queued messages.
- `/peers-name [name]`: show or set this session's name. Any Unicode text works; names are unique among live sessions.
- `/peers-inbox`: messages received from other sessions, including ones still queued.
- `/peers-outbox`: messages this session sent, and whether each is queued, delivered or read.

Agents with `can_spawn` (the `build` agent by default) can start background sessions, give them a task and close them. Watch them in split view.

## Permissions and delays

When a tool call needs permission, the TUI shows the call and asks:

- `a`: allow this call
- `s`: allow every call of this tool for the rest of the session
- `d`: deny; the model is told and continues

When a tool has a delay, a countdown is shown before it runs, and `esc` cancels the call. Both are set in the configuration: see [Permissions](configuration.md#permissions) and [Tool delays](configuration.md#tool-delays).
