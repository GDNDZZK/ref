**English** · [简体中文](gui.zh-CN.md)

# Desktop GUI

- [Installing](#installing)
- [Starting](#starting)
- [The window](#the-window)
- [Split view](#split-view)
- [The conversation](#the-conversation)
- [The input box](#the-input-box)
- [Notifications](#notifications)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Settings](#settings)
- [Known limits](#known-limits)
- [Files and backend](#files-and-backend)
- [Building and developing](#building-and-developing)

`ref-gui` shows ref in a desktop window (Qt Quick). Like `ref` and `ref-web` it only displays sessions: they run in `ref-server`, so a session can be open in the terminal, the GUI and a browser tab at the same time. When no backend is running, `ref-gui` starts the `ref-server` that sits in its own folder; a backend started this way exits when the last client disconnects.

## Installing

Download `ref-gui-<version>-<platform>` (or `ref-all-…`) from the [latest release](https://github.com/GDNDZZK/ref/releases/latest); [Installation](../README.md#installation) in the README lists the platforms and what each system needs. The archive holds one folder:

| System | Contents |
|---|---|
| Linux | `ref-gui`, `ref-server`, `lib/`, `plugins/` and `qml/` (Qt and the other libraries), `qt.conf`, `ref-gui.desktop`, `ref.svg` |
| macOS | `ref.app`, which holds `ref-server` and all libraries |
| Windows | `ref-gui.exe`, `ref-server.exe`, the Qt and Visual C++ runtime DLLs, `plugins\` and `qml\` |

Keep the folder together and run the program from where it is; moving the whole folder is fine. No installation or administrator rights are needed. Everything the GUI needs besides the system's own libraries is in the folder, so it does not use a Qt that is installed on the system.

On Linux the program needs glibc 2.38 and the C++ runtime of GCC 13 (Ubuntu 24.04, Debian 13, Fedora 39, Arch and newer). It runs natively on Wayland (selected automatically when `WAYLAND_DISPLAY` is set) and falls back to X11 or XWayland.

## Starting

```bash
ref-gui                       # open in the current directory (no session started yet)
ref-gui ~/src/project         # open with another default directory
ref-gui -c                    # continue the most recent session for this directory
ref-gui -s ses_01a1…          # open a session by id (live or from history)
ref-gui -w                    # start in the split view
ref-gui --agent plan --model openai/gpt-5#high
```

| Option | Meaning |
|---|---|
| `DIR` | Default directory for new sessions (default: the current directory). |
| `-c`, `--continue` | Continue the most recent session for this directory. |
| `-s`, `--session ID` | Open a session by id, live or from history. |
| `-w`, `--watch` | Start in the [split view](#split-view) of the live background sessions. |
| `--agent NAME` | Default agent for new sessions. |
| `--model provider/model[#variant]` | Default model for new sessions. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token for `--server`. Needs `--server`. |
| `--mock` | Use a private fake backend with canned scripts (for development, see [The mock backend](development.md#the-mock-backend)). Cannot be combined with `--server`. |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

Without `-c` or `-s`, `ref-gui` opens in the empty workspace state (*No session open*) without creating a blank session (`--smoke` prints `ready` when no session flag is passed, or `attached <id>` with `-c` / `-s`). The window title is `ref`. On Windows `ref-gui.exe` is a window-only program: it opens no console, so `--help` and `--version` print nothing there.

## The window

The window has a sidebar on the left and the content on the right: one session, the empty workspace (*No session open*, with *New session in folder…* and *New session*), or the [split view](#split-view).

- **Running** lists the live sessions of this window (newest first by default). Long-press (~280 ms) and drag a row vertically to reorder live sessions (auto-scrolling when needed); newly created sessions appear at the top, and the custom order is remembered (`sidebar.runningOrder` in `gui.json`). **History** lists the sessions the backend has saved, by time of the last change; the search box filters the 200 most recent ones. A click opens a session; clicking the already-open session again in single view, choosing *Move to background* from its right-click menu, or running `/detach` (`/background`, `/leave`, or Command Palette → *Move session to background*) detaches it to the background without stopping it and returns to *No session open*. Double-click a running session, or use the right-click menu, to rename or close it. The right-click menu of a history entry deletes it.
- A running session is labelled with the name you gave it, else its title, else the name of its directory. A session in the history is labelled with its title (on one line), else its name, else the name of its directory, as in the terminal UI. The *Running* list is as tall as its rows and scrolls only when *History* would get fewer than three rows.
- The *New session* button creates a session in the workspace's default directory. The folder button next to it, or *New session in folder…* / *New session in another folder…*, asks for a directory first and updates the workspace's default directory to that folder.
- Closing a session that is still working asks for confirmation first (including from the sidebar and the *Sessions* dialog); its conversation stays in history. Closing the last open session returns to *No session open* instead of starting a new one (and in split view, closing the held foreground session clears it without pulling a background tile out of the grid).
- The sidebar can be hidden and shown again (Ctrl+Shift+B) and resized by dragging its right border (210–480 px; double-click the edge to reset to 264 px). Click a *Todo*, *Running*, or *History* header (or press Space / Return / Enter when focused) to collapse or expand that section; collapsed sections give their vertical space to the remaining expanded ones. Drag the horizontal divider between *Todo* and *Running* or between *Running* and *History* vertically to adjust section heights (double-click a divider to return to automatic sizing). Sidebar visibility, width, section fold states, and custom section heights are remembered.
- At the top of the sidebar, above *Running*, the *Todo* section shows the to-do list of the session you are looking at (in the split view, of the focused tile) with the number of items done, like *Todo 2/5*; cancelled items count in neither number. Long items wrap, the list scrolls once it reaches its height limit (a third of the sidebar by default, or the custom dragged height), and the section is hidden while the session has no to-do list.
- The command palette (Ctrl+P) lists every command and filters as you type. Commands that take no argument run at once; choosing *Rename the session*, *Change this session's working directory*, *Export transcript as markdown*, *Set or cycle model variant* or *Close live sessions* opens its dedicated dialog directly, and pressing Esc while entering custom-command arguments returns to the command list. It holds the same commands as the `/` list in the input box, including your [custom commands](configuration.md) (group *Custom*). Without a shortcut: *New session in another folder…*, *Move session to background* (`/detach`), *Rename the session*, *Change this session's working directory*, *Close live sessions*, the `peers` commands, *MCP servers*, *Configuration* (`/config`) and *Reload configuration* (`/reload`). There are no `/editor` and `/exit` commands.
- *Configuration* (`/config` or `/configuration`, the configuration button in the sidebar footer, or *Settings* → *Edit ref.json…*) opens the 860 px six-section visual and raw JSONC editor for global `ref.json`, with multi-profile switching (`ref.json.<n>.bak`) and Ctrl+S to save; `/reload` (`/reload-config`, or the reload button) hot-reloads configuration across live sessions and MCP servers. See [Graphical editor and profiles](configuration.md#graphical-editor-and-profiles).
- If the running backend is older than the GUI, a banner at the top of the content offers *Restart backend*. The button is disabled while a session is running or waiting for permission. The restart stops the backend; the GUI then starts the installed one again.
- A message in the corner reports errors of sessions you are not looking at and the result of actions such as an export.
- The *Sessions* dialog (Ctrl+Shift+L or `/sessions`) filters between *All*, *User-created*, and *By creator* of the selected row, marks spawned sessions with their creator, and searches creator names and ids alongside session names, titles, and directories.

## Split view

The split view (Ctrl+Shift+W, the button at the top right, or `ref-gui -w`) shows one tile per background session: the live sessions that no other client has open, excluding the current foreground session (which stays hidden until you leave split view). Split view starts with no tile focused; click a tile or press Alt+arrow keys to focus it, and click the focused tile's header or empty background to deselect. Tiles spawned by another session show a `⟲ <root>` chip in the header and tint unfocused borders and status dots by their root task; user-created tiles stay unmarked. Only the focused tile shows its input box (as in the terminal UI), so the others have room for the conversation; they keep the status line and the queue of waiting messages, and a draft, attachments and shell mode survive while the box is hidden. The sidebar selects the session of the focused tile, and marks all tiles with a bar; commands act on the focused tile. Double-click a tile's title bar, or use its zoom button (Ctrl+Shift+Z), to enlarge it. Leave the zoom with Ctrl+Shift+Z or the *Leave zoom* button at the top right. Esc does not leave it, because Esc belongs to the input box, to permission questions and to the model's questions. With no background sessions the view says so.

Clicking the *Jump to latest* button on an unfocused tile scrolls it to the bottom without focusing the tile or opening its input box. While resizing split tiles or dragging the sidebar, tile content smoothly blurs and fades out before reflowing at the new width.

## The conversation

- Your prompts, the model's replies, its reasoning and its tool calls appear in one column. Under every message a small line shows who wrote it (agent, model, variant), how long it took and the tokens used (input including cache reads and writes, and output). Interrupted and failed messages are marked. The line shows *Copy* on hover, and *Undo* on your own messages.
- Markdown tables render as styled bordered cards with header shading, horizontal scrolling when wide, a hover *Copy* button, and independent cell text selection.
- The view follows the end of the conversation while the model writes. Scroll up (wheel, touchpad, scroll bar) to stop following; a *Jump to latest* button appears. Scrolling to the end follows again.
- Only the newest messages are loaded when a session opens. Scroll near the top and earlier ones load by themselves; the *Load older messages* row at the top does the same by click, and shows the error with a retry when loading fails.
- After a compaction a divider marks where the model's context starts. Messages above it are dimmed: you can still read them, but the model no longer sees them. The divider's summary folds and unfolds.
- Messages that did not come from you are marked: from another session (*message from "name"*), created by a parent session, a system reminder, or the output of a `!` shell command. When you switch the agent, a reminder tells the model about it.
- Reasoning blocks open while the model thinks and fold when it is done. Click the header to open or fold one; the commands *Toggle expanded thinking* (Ctrl+Shift+R) and *Toggle expanded tool details* (Ctrl+Shift+O) do all of them at once and override the blocks you opened by hand.
- The line above the input box shows what the session is doing (Working, Running tools, Compacting context, Waiting for your permission, Waiting for your answer) with the time of the current turn, a countdown when a request is being retried, and a small bar for how full the model's context is. The bar turns yellow when compaction is recommended and red when it will happen by itself; hover for the numbers. When the session has a to-do list, the line also shows the item being worked on (else the next pending one) and how many items are done, like *Check the workspace · 1/3*, and a green *✓ 3/3* when all are done; cancelled items count in neither number, in the sidebar and tile badges too. A click on it opens the whole list above the line; Esc, a click elsewhere or another session closes it. The conversation's own errors show at its end until you send the next prompt.

**Tool cards**

- Every tool call is a card: a state mark, the tool's icon and name, what it is for, one line from its arguments (command, path, pattern, URL, …), the state, how long it took and when it started. Click the header to open it. Failed calls show the first line of the error when folded. Collapsed `edit` and `write` cards show the path, `+`/`-` counts, the first 8 diff rows (with `+N more`), and any edit warnings; finished `question` cards show a compact summary of the chosen answers when collapsed.
- Open cards show what is useful for the tool: raw Input, the full diff (plus any edit warnings) and Output for edits and writes, the command, output and exit code for bash (a non-zero code is a red label; timeout, cancel and truncation have labels too), numbered lines for read and grep (matches highlighted, gaps marked), the paths for find, status code, type and size for webfetch with *Open in browser*, the recipient and text for messages to other sessions. The to-do list of a `todo` call is always open. Anything else shows its input and output as text.
- Long output is cut to 200 lines with *Show N more lines*; code lines scroll sideways. A bash call that is still running shows its last 20 lines. Output has a *Copy* button.
- A tool that is waiting for a delay shows a ring and a *Cancel* button. *Cancel*, or a single Esc in the input box, stops the turn before the tool runs.

**Permissions**

- A call that needs your permission opens by itself and shows the command or path (for `edit` and `write`, a *Preview* diff computed when the request was created) with three buttons: *Allow* (a), *Allow for session* (s), *Deny* (d). With the session focused you can also press a or Enter, s, d or Esc, also while the cursor is in the input box. The keys only work 350 ms after the request appears, and again 350 ms after a tile gets the focus or is zoomed or another session is shown, so text you were typing, or a click on the tile, does not answer it by accident; until all requests are answered, letter keys do not go into the input box (Ctrl and Alt shortcuts and editing keys still work). When several calls wait, the oldest is answered first and the card says *+N more waiting*. In the split view the keys go to the focused tile.

**Questions**

- When the model asks you something (the `question` tool), a card appears between the conversation and the status line of that session, in the single view and in every split-view tile, focused or not. Its title is `?` and the short heading of the question, followed by the question and the choices. Each choice has a number and its description, the default one is marked *(default)* and preselected. The card grows with its content and scrolls when the pane is too small. The sidebar, the tile header and the status line show *Waiting for your answer* with a question mark icon.
- One question with a single answer is a plain list: a click (or Enter) on a choice answers at once. Several questions, or one with several answers, get a tab per question and a last tab *Confirm*. On a single-answer tab a click picks the choice and moves to the next tab; on a multiple-answer tab the choices are check boxes, the defaults ticked. *Confirm* lists the answers and marks questions without one as *Unanswered*; *Submit* (or Enter) sends them, unanswered questions empty.
- The last choice, *Type your own answer*, is there when the backend takes free text. It turns into a text field: Enter takes the text (empty text takes nothing), Esc goes back to the list. With a single answer the text replaces the picked choice; with several answers it is added, and choosing the row again takes it back.
- *Dismiss*, or Esc, gives no answer and the model carries on without one. While an answer is on its way the card is dimmed (*Sending…*) and goes when the backend reports the question resolved. If the backend refuses the answer, the reason shows in the session's message at the top and the card stays. If another window or the terminal UI answers first, the card disappears without a message.
- The keys, as for permissions: ↑ ↓ or j k move the highlight (it stops at the ends), Enter or Space picks or ticks the highlighted choice, 1 to 9 take a choice by its number, ← → and Tab or Shift+Tab switch tabs, Esc dismisses. They work 350 ms after the question appears (and again after the session gets the focus, as for permissions), with the session focused (in the split view, the focused or zoomed tile), also while the cursor is in the input box. Until the question is answered other typing does not reach the input box (Ctrl and Alt shortcuts and editing keys still work), and a held key does not answer. While you type a custom answer the keys belong to the text field. The mouse works in every tile.

**Images**

- Pictures you attach and pictures a tool returns show as small thumbnails, 72 px high. Click one to see it large over the conversation; Esc, a click outside the picture or the close button put it away and the cursor goes back to the input box. Pictures are fetched from the backend when needed.

**Undo**

- *Undo last prompt* (the button under your message, or Ctrl+Shift+U) takes the conversation back to before that prompt and puts the prompt, with its pictures, back in the input box to edit and send again. The removed messages fade out. Files the model changed are not restored.

**Links and untrusted content**

- Links in messages open in your browser, only for http, https and mailto addresses. Images in the model's Markdown (`![]()`) are shown as links, not loaded, and HTML tags in a message are shown as text. Nothing in a conversation makes the GUI load anything from the network by itself.

## The input box

| Key | Action |
|---|---|
| Enter | Send (or in `/cd`, `/export` and `!` shell mode, run the typed text unless a completion was explicitly selected with ↑/↓). On a standalone `/` completion list, Enter executes the selected command directly. While the session is busy the prompt waits in a queue above the input box. |
| Shift+Enter, Ctrl+J | New line. |
| Tab, Shift+Tab | Next or previous agent (outside shell mode). In `/`, `@` and `!` shell mode, Tab completes (without adding a trailing space on standalone slash commands, and drilling into subdirectories on paths). |
| Ctrl+T | Cycle the model's variants. |
| Esc, Esc | Interrupt the running turn (the second press within 1.5 seconds). A first Esc closes an open list or leaves shell mode (keeping any typed text) instead. |
| ↑, ↓ | Walk through the prompt history (filtered to `!` entries in shell mode): ↑ from an empty box or from the start of the text, and while browsing from the first line; ↓ from the last line while browsing. |
| Ctrl+C | Without a selection, empties the input box (text of 20 characters or more goes to the history first). With a selection it copies. |
| `!` at the start | Shell mode: the line runs as a shell command, with `!` history + `$PATH` binary completion for commands (including after `VAR=val` assignments and command wrappers such as `sudo`, `env`, `nice`, and `timeout`) and file/directory completion for arguments. Backspace on an empty box or Esc leaves it. |

On macOS, Ctrl in this table is Cmd.

- The tags under the box show the agent, the model and the variant. Click one to choose; a tag changes only after the backend has confirmed. Press Ctrl+T to cycle the variant directly, Ctrl+Shift+T to open the variant picker, or click the variant tag.
- `/` triggers command completion both at the start of the box and mid-text after whitespace, matches case-insensitively (including singular aliases `/model` and `/agent`, `/models <m>`, `/agents <a>`, `/variant <v|default>`, `/sessions <s>`, `/close-session <s…>`), and completes arguments for `/cd`, `/export`, `/models`, `/agents`, `/variant`, `/sessions` and `/close-session` (plus `@file` inside custom command arguments). Standalone commands (all built-ins except `/rename`) complete without a trailing space on Tab and run on Enter; unknown single-word `/foo` inputs are blocked with a warning toast and fuzzy suggestion instead of being sent as a prompt. `@` lists files under the session's directory (`.gitignore` is respected) and inserts `@path `.
- Images and files: paste an image from the clipboard (Ctrl+V), paste a file path or `file://` address, or drop any file on the box. Supported images (PNG, JPEG, GIF and WebP, up to 20 MiB each) attach as a thumbnail above the text with a remove button and an atomic `[Image N]` token in the text (numbered per draft from 1 once empty, kept in sync across undo/redo, with the cursor stepping over it, selections expanding across it, and Backspace/Delete removing the whole token), as in the terminal UI; other existing local files insert their path at the cursor.
- The prompt history is the file `prompt-history.jsonl` in the state directory (500 entries) and is shared with the terminal UI. Shell lines are saved with their `!`.

## Notifications

The GUI can show a system notification when a session you are not looking at needs a tool permission, asks a question, finishes a turn, or ends a turn with an error. A session counts as not looked at when it is not on screen, and when the window is not in the foreground. A click on the notification opens the session. The title is the session's title (`ref` when it has none); the text is *Waiting for permission to use {tool}*, *Asks: {question}* (the first question), *Finished* or *Stopped with an error: {error}*.

Under *Settings* a master switch and three kinds (a session waits for you, for a permission or a question; a turn finished; a turn ended with an error) choose what is shown. If the system cannot show notifications, the settings say so and the taskbar icon flashes instead.

| System | Mechanism |
|---|---|
| Linux | The desktop notification service (`org.freedesktop.Notifications`). Not available when no such service runs. |
| Windows | Toast notifications. The program registers its own AppUserModelId under `HKCU`; Windows shows toasts only when the Start menu has a shortcut for ref. |
| macOS | UserNotifications. Available only in the packaged `ref.app`, after you allow notifications for it. |

When another interface is the one you use, only that one is notified: the backend sends each notification to one client.

## Keyboard shortcuts

On macOS, Ctrl in the table is Cmd. The letters are the same in the [Web UI](web.md#keyboard-shortcuts), which uses Alt instead of Ctrl+Shift.

| Command | Shortcut | `/` command |
|---|---|---|
| Command palette | Ctrl+P | |
| New session (same directory) | Ctrl+Shift+N | `/new` |
| Switch session (live and history) | Ctrl+Shift+L | `/sessions` |
| Switch model (this session only) | Ctrl+Shift+M | `/models` (`/model`) |
| Switch agent | Ctrl+Shift+A | `/agents` (`/agent`) |
| Cycle model variant | Ctrl+T | `/variant` |
| Set or cycle the model variant | Ctrl+Shift+T | `/variant` |
| Undo last prompt | Ctrl+Shift+U | `/undo` |
| Close this session | Ctrl+Shift+K | `/close` |
| Export transcript as Markdown | Ctrl+Shift+X | `/export` |
| Copy last assistant message (raw text) | Ctrl+Shift+Y | `/copy` |
| Toggle expanded thinking | Ctrl+Shift+R | `/thinking` |
| Toggle expanded tool details | Ctrl+Shift+O | `/details` |
| Split view | Ctrl+Shift+W | `/watch` |
| Toggle sidebar | Ctrl+Shift+B | `/sidebar` |
| Zoom the focused tile (split view) | Ctrl+Shift+Z | |
| Zoom in interface | Ctrl+= (Ctrl++, Ctrl+Shift+=) | |
| Zoom out interface | Ctrl+- | |
| Reset interface zoom | Ctrl+0 | |
| Keybindings and commands | Ctrl+Shift+H | `/help` |
| Settings | Ctrl+Shift+S | |
| Move the tile focus (split view) | Alt+arrow keys | |

*Set or cycle the model variant* without an argument switches to the next variant. The help dialog (Ctrl+Shift+H) lists the same keys.

## Settings

Open them with Ctrl+Shift+S or the settings button at the bottom of the sidebar.

- **Theme:** System, Light or Dark.
- **Language:** System, English or 简体中文. The conversation area and the dialogs are translated.
- **Interface zoom:** `50%`–`400%` (`−` / `+` / *Reset* buttons, Command Palette, or `Ctrl+=` / `Ctrl+-` / `Ctrl+0` in `5%` steps, or type any percentage directly; `Cmd` on macOS). Restart `ref-gui` after changing zoom for native `QT_SCALE_FACTOR` sharpness.
- **Notifications:** the master switch and the three kinds described [above](#notifications).
- **Configuration:** *Reload configuration* and *Edit ref.json…* (see [Graphical editor and profiles](configuration.md#graphical-editor-and-profiles)).

They are saved in `gui.json` in the state directory ([Files and directories](cli.md#files-and-directories)), together with `uiScale` (interface zoom factor), whether the sidebar is shown, its width, section fold states, custom section heights (`sidebar.todoHeight` / `sidebar.runningHeight`), running session order (`sidebar.runningOrder`), and the size of the window:

```json
{
  "theme": "system",
  "language": "system",
  "uiScale": 1,
  "notifications": { "enabled": true, "permission": true, "turnDone": true, "turnError": true },
  "sidebar": { "visible": true, "todoExpanded": true, "runningExpanded": true, "historyExpanded": true, "width": 264, "todoHeight": 0, "runningHeight": 0, "runningOrder": [] }
}
```

`theme` is `system`, `light` or `dark`; `language` is `system`, `en` or `zh_CN`; `uiScale` is `0.5`–`4` (default `1`); `sidebar.todoHeight` and `sidebar.runningHeight` are `0` (automatic) or a height in pixels. The file is written on every change. If it cannot be read, it is renamed to `gui.json.bad`, the defaults are used, and the settings dialog and a message in the corner say so. Values that are wrong are ignored one by one and keep their defaults.

## Known limits

- Notifications on Windows and macOS have not been tried on those systems yet.
- On Wayland, whether a click on a notification brings the window to the front depends on the desktop: without an activation token the taskbar entry may only flash.
- Ctrl+Shift+U can be taken by the IBus Unicode input on Linux. If the key does not respond, turn that IBus key off, or use the *Undo* button under your message or `/undo`.
- **Input methods (IME):** On Linux, the package bundles `fcitx5-qt` alongside Qt's `ibus` and `compose` input context plugins, so `fcitx5` (`QT_IM_MODULE=fcitx`) and `IBus` work out of the box on both native Wayland and `xcb` (X11/XWayland). On native Wayland without `QT_IM_MODULE` set, Qt uses `zwp_text_input_v3`.
- **Wayland window icon:** Wayland compositors resolve window icons by desktop ID, so installing `ref-gui.desktop` and `ref.svg` (see [Installation](../README.md#installation)) is required for the window icon to appear.
- In very long sessions (more than about 200 messages above or below the view) the scroll bar length is an estimate, and may shift a little while you drag it.
- A bash call that exits with a non-zero code still shows as done in the card header; the exit code is in the open card.
- For tools the GUI has no special view for, the one-line summary lists the first three plain arguments in alphabetical order, where the terminal UI uses the order in the call.
- Tool arguments and output are shown as UTF-8 text.
- There is no `/editor` and no `/exit` command, and the GUI does not quit when the last session closes.

## Files and backend

The GUI uses the same instance files, configuration and session storage as `ref` ([Files and directories](cli.md#files-and-directories)), and the same environment variables.

## Building and developing

See [The GUI](development.md#the-gui) in the development guide.
