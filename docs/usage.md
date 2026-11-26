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
- [Questions from the model](#questions-from-the-model)

## Starting

```bash
ref                       # open the empty welcome screen in the current directory
ref ~/src/project         # open with another default directory
ref -c                    # continue the most recent session for this directory
ref -s ses_01a1…          # open a session by id (live or from history)
ref -w                    # split view of all background sessions
ref --agent plan          # pre-select an agent for the new session
ref --model openai/gpt-5#high -p "Explain src/main.rs"
```

All flags are listed in [Command line](cli.md#ref).

## Sessions and the backend

ref has two programs. `ref-server` (the backend) runs the sessions: it calls the LLM, runs tools, stores history and handles permissions. `ref` (the TUI) shows sessions and sends your input. They talk over a local WebSocket.

When you start `ref`, it resolves the backend from `--server`, then any saved default in `~/.local/state/ref/tui.json`, and otherwise connects to a running local backend or lazily starts `ref-server`. A backend started locally this way exits when the last `ref` window closes. Other programs that show sessions, such as the desktop GUI ([Desktop GUI](gui.md)) and browser UI ([Web UI](web.md)), find the same local backend through the same instance file, so a session can be open in several of them at once. A backend started by one of them exits after its last client of any kind disconnects, and the turns that are running stop with it. While at least one client is connected, sessions keep running even if no window shows them, so you can close one window, open another and continue where the session is. To keep sessions running with no window open, start the backend yourself with `ref-server` (see [Command line](cli.md#ref-server)); a backend started that way never exits on its own.

- Without `-c`, `-s`, `-w`, or `-p`, `ref` starts on an empty welcome screen without creating a session (`-c` with no saved session in the directory warns instead of creating one). On the welcome screen you can pre-select the working directory (`/cd`), agent (`tab`, `/agents`), model (`ctrl+x m`, `/models`), and variant (`ctrl+t`, `/variant`) before sending your first prompt, or run `/new` (`ctrl+x n`) to create a session immediately.
- A session is **foreground** while a frontend is attached to it, and **background** otherwise. Clicking the active session row in the sidebar or running `/detach` (`/background`, `/leave`) detaches it to the background and returns to the empty welcome screen.
- Sessions are saved in `~/.local/share/ref/sessions/`. `ctrl+x l` lists live sessions and history; opening an old one loads it again.
- Several windows can show the same session at the same time.

## The screen

- **Welcome screen / Transcript**: when no session is open, shows the current directory, active backend summary, and a clickable `Switch Backend…` button; when a session is open, shows your prompts, the model's replies, reasoning and tool calls.
- **Input box** at the bottom, with the agent, model and variant next to it.
- **Sidebar** (`ctrl+x b` toggles it):
  - **Live sessions**, each with its todo progress (`2/5`, or a green `✓3/3` when all are done; cancelled items are not counted) and, while busy, how long the current turn has been running. Clicking the active row detaches to the empty welcome screen.
  - **MCP servers** and their status.
- **Status line**: what the session is doing and the turn's running time. The time comes from the backend, so it is correct after switching sessions. When retrying a failed request, clicking the retrying status line retries immediately.

## Writing prompts

| Input | Effect |
|---|---|
| `enter` | Send. |
| `shift+enter`, `ctrl+j` | New line. |
| `/` | Command list and argument completions (see [Slash commands](#slash-commands)). |
| `@` | Fuzzy file list; the chosen path is inserted (also inside custom command arguments). |
| `!command` | Shell mode: run the command in the session directory (10-minute limit). The command and its output are added to the conversation as your message, without an LLM turn. |
| `ctrl+v` | Paste an image from the clipboard. Pasting the path of an image file attaches it too (outside shell mode). |
| `ctrl+x e` | Write the prompt in `$VISUAL` / `$EDITOR` (default `vi`, `notepad` on Windows). |
| `↑` / `↓` | Prompt history, when the input box is empty or the cursor is at its boundary (filtered to `!` commands in shell mode). |
| `esc esc` | Interrupt the session. |

**Slash completions and typo guard.** Slash commands match case-insensitively and complete arguments for `/cd`, `/export`, `/models`, `/agents`, `/variant`, `/sessions`, `/close-session` and `/mcp` (plus `@file` inside custom command arguments). Standalone slash commands (all built-ins except `/rename`, plus custom commands) complete without a trailing space on `tab` and run directly on `enter`. An unknown single-word `/foo` is blocked with a warning toast and the closest command suggestion instead of being sent to the model, keeping your draft intact.

**Shell mode (`!`).** Typing `!` in an empty box enters shell mode (`backspace` on an empty box or `esc` leaves it, keeping any typed text). Command tokens (at the start of the line, after `|`, `&&`, `||`, `;`, or after leading `VAR=val` assignments and command wrappers such as `sudo`, `env`, `nice`, and `timeout`) complete from `!` prompt history and `$PATH` executables, or from paths when the token contains `/` or `\` (or starts with `.`, `~` or a Windows drive letter); argument tokens complete files and directories in the session directory, and `tab` on a directory appends `/` and drills into the next level. In shell mode `tab` and `shift+tab` never cycle the agent, and `↑` / `↓` browse `!` history only. In `/cd`, `/export` and `!` shell mode, `enter` runs what you typed unless you explicitly selected a completion with `↑` / `↓` (`tab` always accepts).

**Queued prompts.** A prompt sent while the session is busy waits under the transcript and is delivered at the next safe point, after the current tool step. Click `✕ remove` to take it back: its text and images return to the input box (after a blank line if you were typing). If it was delivered in the meantime, a toast says so. Messages queued by other sessions and reminders are listed too, labelled with their sender, but cannot be removed. Every window attached to the session sees the queue change live.

**Undo.** `ctrl+x u`, `/undo`, or `↶ undo` under a prompt removes that prompt and everything after it, and puts the prompt's text and images back into the input box. Files changed by tools are **not** restored.

## Reading the transcript

- Every message shows its time: your prompts, each reply (start, end, duration and tokens) and each tool call (start and duration).
- A collapsed tool call shows the tool name and the `purpose` the model gave. Click it to see the input and output. `ctrl+x d` expands all of them.
- Collapsed reasoning shows its latest characters on one line, updated live. `ctrl+x t` expands it.
- The view follows the end of the conversation while new output arrives. Scroll up (mouse wheel, `page up` or `shift+↑`) to stop following; scrolling back to the bottom (`end` or `ctrl+alt+g`) follows again.
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
| `/sessions [name or id]` (`/resume`, `/continue`) | Switch session (picker, or directly by name, title or id) |
| `/models [model]` (`/model`), `/agents [agent]` (`/agent`) | Pick or switch the model or agent |
| `/variant [name]` (`/variants`, `/effort`) | Pick, set or reset (`default`) the model variant (`ctrl+t` cycles in TUI) |
| `/compact` (`/summarize`) | Compact the conversation |
| `/undo` | Undo the last prompt |
| `/rename NAME` | Rename the session |
| `/cd [DIR]` (`/cwd`) | Change the session's directory (opens a directory picker when omitted) |
| `/detach` (`/background`, `/leave`) | Move the open session to the background (TUI, `ref-gui`, and `ref-web`) |
| `/retry` | Retry immediately (skip the retry backoff wait; clicking the retrying status line does the same) |
| `/stash [name]` | Stash all live sessions into a new group (TUI, `ref-gui`, and `ref-web`) |
| `/stashed` (`/groups`, `/stash-groups`) | Open the Stash Groups manager (`ref-web` uses `/stashed` or `/groups`) |
| `/backend` (`/server`) | Switch backend connection interactively, or pass `auto` / `URL [TOKEN]` temporarily in TUI |
| `/window [N\|reset]` | Set resident message window size in TUI (`1000` default, `0` = unlimited / load all), saved in `tui.json` |
| `/close` | Close this session |
| `/close-session [name or id …]` (`/kill`) | Close live sessions (picker, or one or more by name or id) |
| `/export [PATH]` | Export the transcript as markdown (default `ref-session-<id>.md` in the session directory) |
| `/copy` | Copy the last reply |
| `/thinking`, `/details`, `/sidebar` | Toggles |
| `/watch` (`/split`) | Split view |
| `/mcp [server]` (`/mcps`) | MCP servers: status, errors and tools (`r` refreshes in TUI) |
| `/config` (`/configuration`) | Graphical `ref.json` editor and profile switcher (`ref-gui` and `ref-web`) |
| `/reload` (`/reload-config`) | Reload configuration across live sessions and MCP servers |
| `/editor` | Write the prompt in `$EDITOR` (TUI) |
| `/peers`, `/peers-name`, `/peers-inbox`, `/peers-outbox` | See [Working with several sessions](#working-with-several-sessions) |
| `/help` | Keybindings and commands |
| `/exit` (`/quit`, `/q`) | Quit (TUI) |

Your own commands from the configuration are listed too (see [Custom commands](configuration.md#custom-commands)).

**`/cd [DIR]`** moves the session (or the pre-selected directory before a session is created) to another directory: absolute, relative to the current one, or `~/…`. Without `DIR`, it opens an interactive directory picker (`tab` drills into subdirectories, `backspace` at a trailing `/` steps up). For an open session, project configuration and context files are loaded again, and a note telling the model about the change is added to the conversation, without an LLM turn. It is not allowed while the session is busy. New sessions from this window start in the new directory too.

**Switching the agent** (`tab`, `ctrl+x a` or `/agents`) adds a note to the conversation that tells the model about the change, without an LLM turn. Several quick switches share one note, and switching back to the original agent removes it. A session with no conversation yet gets no note; only the agent changes. While the session is busy the note is added right before the next request.

**`/close`** closes the current session and returns to the empty welcome screen without quitting `ref` or auto-creating a replacement session (press `ctrl+x q` or run `/exit` to quit).

**`/stashed`** opens the two-pane Stash Groups dialog (groups on the left, members on the right; restoring a group or member brings it back as a background live session):

| Key | Action |
|---|---|
| `tab`, `←` / `→` | Switch between Groups and Members panes |
| `↑` `↓`, `j` `k`, `ctrl+p` / `ctrl+n` | Move selection |
| `enter` | Groups pane: open Members pane (or restore if empty); Members pane: restore member (double-click restores group or member) |
| `R` | Restore the entire selected group |
| `c` / `s` / `r` / `l` | Create empty group / Stash all live sessions / Rename group / Lock or unlock group |
| `m` / `a` | Merge group (Groups pane) or move member (Members pane) / Add a live session to the group |
| `shift+↑` / `shift+↓`, `K` / `J` | Reorder member within the group (Members pane) |
| `x`, `delete`, `ctrl+d` | Delete group (press twice to confirm) or remove member |
| `g` | Refresh groups |
| `esc`, `q`, `ctrl+c` | Close dialog (or cancel confirmation, input, or target picker) |

**`/backend`** opens a dialog to switch endpoints temporarily (`enter`), save one as default in `~/.local/state/ref/tui.json` (`ctrl+s`), or restore local auto-discovery (`ctrl+r`).

## Split view

`ref -w`, `ctrl+x w` or `/watch` shows the live background sessions side by side.

- The current foreground session of this window is hidden while split view is on and returns when you leave. Split view starts with no tile focused; click a tile, or press `tab` or `alt+arrows`, to move the focus (click the focused tile's header or empty space to clear focus).
- A direct-parent creator filter pill (`All`, `User-created`, or `By <creator>`) sits in the status line: press `f` (while no tile is focused) or click the pill to cycle filters.
- Tiles use solid rounded borders (`╭─╮│╰─╯`). Sub-sessions spawned by another session show a `⟲ <root>` chip in the header and tint unfocused borders and chips using a deterministic 8-color root-task palette; tiles waiting for a permission or `question` answer highlight in warning yellow.
- Typing and `enter` send a message to the focused session, `esc esc` interrupts it and `ctrl+o` zooms in. Slash commands, `@file` mentions, `!` shell mode (with full autocompletion above the focused input), and `enter` on standalone slash completions act on that tile's session (`tab` cycles focus between tiles unless a completion popup is open or shell mode is active).
- Each tile shows its queued prompts (`✕ remove` takes one back); clicking a retrying tile header retries immediately. Clicking a tool call shows its input and output inside the tile; clicking again collapses it. Dragging across a tile's transcript selects text (`ctrl+c` or right click copies; double click zooms).
- The mouse wheel scrolls the tile under the pointer.

## Working with several sessions

Sessions can talk to each other. The model has tools for it (see [Tools](tools.md#cross-session-tools)), and you have commands that run in the backend without an LLM turn:

- `/peers` (`/list-agents`): other live sessions with their status, agent, foreground or background, directory and queued messages.
- `/peers-name [name]`: show or set this session's name. Any Unicode text works; names are unique among live sessions.
- `/peers-inbox`: messages received from other sessions, including ones still queued.
- `/peers-outbox`: messages this session sent, and whether each is queued, delivered or read.

Agents with `can_spawn` (the `build` agent by default) can start background sessions, resume closed history sessions, give them a task and close them. Watch them in split view.

## Permissions and delays

When a tool call needs permission, the TUI shows the call and asks:

- `a`: allow this call
- `s`: allow every call of this tool for the rest of the session
- `d`: deny; the model is told and continues

For `edit` and `write`, the request includes a preview diff of what will change before you answer (computed when the request is created; the file may change if you wait).

When a tool has a delay, a countdown is shown before it runs, and `esc` cancels the call. Both are set in the configuration: see [Permissions](configuration.md#permissions) and [Tool delays](configuration.md#tool-delays).

## Questions from the model

The model can ask you to choose with the [`question` tool](tools.md#question). The TUI shows a dialog and the session waits for your answer; a session with an open question shows `? waiting for your answer` in the status line, in every window. Background sessions ask too: they show up in the session list like a pending permission and send a notification. Like a permission request, the dialog ignores `enter`, digits and `esc` for a moment after it opens, so a key you were typing does not answer it.

For one single-choice question the dialog is a list:

| Key | Action |
|---|---|
| `↑` `↓`, `j` `k` | Move the cursor. It starts on the option marked `(default)`, or the first option. |
| `enter` | Take the option under the cursor and send the answer. |
| `1`–`9` | Take that row and send the answer. |
| `esc` | Dismiss the question; the model continues without an answer. |

With several questions or a multiple choice, a tab bar shows each question and a final **Confirm** tab:

| Key | Action |
|---|---|
| `tab`, `→` / `shift+tab`, `←` | Next / previous tab. |
| `enter` | On a single choice: take the option and go to the next tab. On a multiple choice: tick or untick the option (`space` and the row's digit do the same). On **Confirm**: send all answers. |
| `esc` | Dismiss all questions. |

Options marked `(default)` are ticked at the start of a multiple choice. Tabs you have not answered are dimmed, and **Confirm** lists them as `(not answered)`; you can still send. A click on a row or a tab works like the keys.

When a question allows it, the last row is **Type your own answer**. Choose it to type in the row; `enter` keeps the text (on a lone question it also sends the answer), an empty text cancels, and `esc` leaves the field without dismissing the dialog. On a multiple choice the typed text is added to the ticked options, and choosing the row again takes it back. If another client answers first, the dialog closes by itself.

If a client and `ref-server` speak different protocol versions, `ref-server` refuses the connection with a message saying which side to update or how to restart the backend (`ref-server --stop`, then start again).
