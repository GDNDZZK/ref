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
| Linux | `ref-gui`, `ref-server`, `lib/`, `plugins/` and `qml/` (Qt and the other libraries), `qt.conf`, `ref-gui.desktop`, `ref.png` |
| macOS | `ref.app`, which holds `ref-server` and all libraries |
| Windows | `ref-gui.exe`, `ref-server.exe`, the Qt and Visual C++ runtime DLLs, `plugins\` and `qml\` |

Keep the folder together and run the program from where it is; moving the whole folder is fine. No installation or administrator rights are needed. Everything the GUI needs besides the system's own libraries is in the folder, so it does not use a Qt that is installed on the system.

On Linux the program needs glibc 2.38 and the C++ runtime of GCC 13 (Ubuntu 24.04, Debian 13, Fedora 39, Arch and newer), and it runs with X11 or through XWayland. Qt's Wayland plugin is not included.

## Starting

```bash
ref-gui                       # new session in the current directory
ref-gui ~/src/project         # new session in another directory
ref-gui -c                    # continue the most recent session for this directory
ref-gui -s ses_01a1…          # open a session by id (live or from history)
ref-gui -w                    # start in the split view
ref-gui --agent plan --model openai/gpt-5#high
```

| Option | Meaning |
|---|---|
| `DIR` | Directory for the new session (default: the current directory). |
| `-c`, `--continue` | Continue the most recent session for this directory. |
| `-s`, `--session ID` | Open a session by id, live or from history. |
| `-w`, `--watch` | Start in the [split view](#split-view) of the live background sessions. |
| `--agent NAME` | Agent for the new session. |
| `--model provider/model[#variant]` | Model for the new session. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token for `--server`. Needs `--server`. |
| `--mock` | Use a private fake backend with canned scripts (for development, see [The mock backend](development.md#the-mock-backend)). Cannot be combined with `--server`. |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

The window title is `ref`. On Windows `ref-gui.exe` is a window-only program: it opens no console, so `--help` and `--version` print nothing there.

## The window

The window has a sidebar on the left and the content on the right: one session, or the [split view](#split-view).

- **Running** lists the live sessions of this window, the newest first. **History** lists the sessions the backend has saved, by time of the last change; the search box filters the 200 most recent ones. A click opens a session. Double-click a running session, or use the right-click menu, to rename or close it. The right-click menu of a history entry deletes it.
- A running session is labelled with the name you gave it, else its title, else the name of its directory. A session in the history is labelled with its title (on one line), else its name, else the name of its directory, as in the terminal UI. The *Running* list is as tall as its rows and scrolls only when *History* would get fewer than three rows.
- The *New session* button creates a session in the directory of the current one. The folder button next to it, or the command *New session in another folder…*, asks for a directory first.
- Closing a session that is still working asks first; its conversation stays in history. When you close the last session, a new one starts in the same directory, so the window is never empty.
- The sidebar can be hidden and shown again (Ctrl+Shift+B). Its state is remembered.
- The command palette (Ctrl+P) lists every command and filters as you type. Commands that take no argument run at once; the others (rename, change directory, close a session by name, peers) ask for it. It holds the same commands as the `/` list in the input box, including your [custom commands](configuration.md) (group *Custom*). Without a shortcut: *New session in another folder…*, *Rename the session*, *Change this session's working directory*, *Close live sessions*, the `peers` commands and *MCP servers*. There are no `/editor` and `/exit` commands.
- If the running backend is older than the GUI, a banner at the top of the content offers *Restart backend*. The button is disabled while a session is running or waiting for permission. The restart stops the backend; the GUI then starts the installed one again.
- A message in the corner reports errors of sessions you are not looking at and the result of actions such as an export.

## Split view

The split view (Ctrl+Shift+W, the button at the top right, or `ref-gui -w`) shows one tile per background session: the live sessions that no other client has open, plus the current session. Only the focused tile shows its input box (as in the terminal UI), so the others have room for the conversation; they keep the status line and the queue of waiting messages, and a draft, attachments and shell mode survive while the box is hidden. A click focuses a tile and shows its input box, and Alt+arrow keys move the focus; the sidebar selects the session of the focused tile, and marks all tiles with a bar; commands act on the focused tile. Double-click a tile's title bar, or use its zoom button (Ctrl+Shift+Z), to enlarge it. Leave the zoom with Ctrl+Shift+Z or the *Leave zoom* button at the top right. Esc does not leave it, because Esc belongs to the input box and to permission questions. With no background sessions the view says so.

## The conversation

- Your prompts, the model's replies, its reasoning and its tool calls appear in one column. Under every message a small line shows who wrote it (agent, model, variant), how long it took and the tokens used (input including cache reads and writes, and output). Interrupted and failed messages are marked. The line shows *Copy* on hover, and *Undo* on your own messages.
- The view follows the end of the conversation while the model writes. Scroll up (wheel, touchpad, scroll bar) to stop following; a *Jump to latest* button appears. Scrolling to the end follows again.
- Only the newest messages are loaded when a session opens. Scroll near the top and earlier ones load by themselves; the *Load older messages* row at the top does the same by click, and shows the error with a retry when loading fails.
- After a compaction a divider marks where the model's context starts. Messages above it are dimmed: you can still read them, but the model no longer sees them. The divider's summary folds and unfolds.
- Messages that did not come from you are marked: from another session (*message from "name"*), created by a parent session, a system reminder, or the output of a `!` shell command. When you switch the agent, a reminder tells the model about it.
- Reasoning blocks open while the model thinks and fold when it is done. Click the header to open or fold one; the commands *Toggle expanded thinking* (Ctrl+Shift+R) and *Toggle expanded tool details* (Ctrl+Shift+O) do all of them at once and override the blocks you opened by hand.
- The line above the input box shows what the session is doing (Working, Running tools, Compacting context, Waiting for your permission) with the time of the current turn, a countdown when a request is being retried, and a small bar for how full the model's context is. The bar turns yellow when compaction is recommended and red when it will happen by itself; hover for the numbers. The conversation's own errors show at its end until you send the next prompt.

**Tool cards**

- Every tool call is a card: a state mark, the tool's icon and name, what it is for, one line from its arguments (command, path, pattern, URL, …), the state, how long it took and when it started. Click the header to open it. Failed calls show the first line of the error when folded.
- Open cards show what is useful for the tool: a diff for edits and writes, the command, output and exit code for bash (a non-zero code is a red label; timeout, cancel and truncation have labels too), numbered lines for read and grep (matches highlighted, gaps marked), the paths for find, status code, type and size for webfetch with *Open in browser*, the recipient and text for messages to other sessions. The to-do list of a `todo` call is always open. Anything else shows its input and output as text.
- Long output is cut to 200 lines with *Show N more lines*; code lines scroll sideways. A bash call that is still running shows its last 20 lines. Output has a *Copy* button.
- A tool that is waiting for a delay shows a ring and a *Cancel* button. *Cancel*, or a single Esc in the input box, stops the turn before the tool runs.

**Permissions**

- A call that needs your permission opens by itself and shows the command or path with three buttons: *Allow* (a), *Allow for session* (s), *Deny* (d). With the session focused you can also press a or Enter, s, d or Esc, also while the cursor is in the input box. The keys only work 350 ms after the request appears, so text you were typing does not answer it by accident; until all requests are answered, letter keys do not go into the input box (Ctrl and Alt shortcuts and editing keys still work). When several calls wait, the oldest is answered first and the card says *+N more waiting*. In the split view the keys go to the focused tile.

**Images**

- Pictures you attach and pictures a tool returns show as small thumbnails, 72 px high. Click one to see it large over the conversation; Esc, a click outside the picture or the close button put it away and the cursor goes back to the input box. Pictures are fetched from the backend when needed.

**Undo**

- *Undo last prompt* (the button under your message, or Ctrl+Shift+U) takes the conversation back to before that prompt and puts the prompt, with its pictures, back in the input box to edit and send again. The removed messages fade out. Files the model changed are not restored.

**Links and untrusted content**

- Links in messages open in your browser, only for http, https and mailto addresses. Images in the model's Markdown (`![]()`) are shown as links, not loaded, and HTML tags in a message are shown as text. Nothing in a conversation makes the GUI load anything from the network by itself.

## The input box

| Key | Action |
|---|---|
| Enter | Send. While the session is busy the prompt waits in a queue above the input box. |
| Shift+Enter, Ctrl+J | New line. |
| Tab, Shift+Tab | Next or previous agent. With the `/` or `@` list open, Tab completes. |
| Esc, Esc | Interrupt the running turn (the second press within 1.5 seconds). A first Esc closes an open list or leaves shell mode instead. |
| ↑, ↓ | Walk through the prompt history: ↑ from an empty box or from the start of the text, and while browsing from the first line; ↓ from the last line while browsing. |
| Ctrl+C | Without a selection, empties the input box (text of 20 characters or more goes to the history first). With a selection it copies. |
| `!` at the start | Shell mode: the line runs as a shell command. Backspace on an empty box or Esc leaves it. |

On macOS, Ctrl in this table is Cmd.

- The tags under the box show the agent, the model and the variant. Click one to choose; a tag changes only after the backend has confirmed. There is no Ctrl+T for the variant: use Ctrl+Shift+T or click the variant tag.
- `/` lists the commands, the same as the command palette, custom commands included. `@` lists files under the session's directory (`.gitignore` is respected) and inserts `@path `.
- Images: paste from the clipboard (Ctrl+V), paste the path or `file://` address of an image file, or drop image files on the box. PNG, JPEG, GIF and WebP up to 20 MiB each. Each picture is a thumbnail above the text with a remove button, and an `[Image N]` token in the text, as in the terminal UI.
- A prompt sent while the session is busy waits in the queue strip above the box. *Remove* on your own prompt takes it back into the box with its pictures; if the backend has already taken it, the GUI says so.
- The prompt history is the file `prompt-history.jsonl` in the state directory (500 entries) and is shared with the terminal UI. Shell lines are saved with their `!`.

## Notifications

The GUI can show a system notification when a session you are not looking at needs a tool permission, finishes a turn, or ends a turn with an error. A session counts as not looked at when it is not on screen, and when the window is not in the foreground. A click on the notification opens the session. The title is the session's title (`ref` when it has none); the text is *Waiting for permission to use {tool}*, *Finished* or *Stopped with an error: {error}*.

Under *Settings* a master switch and three kinds (a tool needs permission, a turn finished, a turn ended with an error) choose what is shown. If the system cannot show notifications, the settings say so and the taskbar icon flashes instead.

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
| Switch model (this session only) | Ctrl+Shift+M | `/models` |
| Switch agent | Ctrl+Shift+A | `/agents` |
| Set or cycle the model variant | Ctrl+Shift+T | `/variant` |
| Compact the conversation (no confirmation) | Ctrl+Shift+C | `/compact` |
| Undo last prompt | Ctrl+Shift+U | `/undo` |
| Close this session | Ctrl+Shift+K | `/close` |
| Export transcript as Markdown | Ctrl+Shift+X | `/export` |
| Copy last assistant message (raw text) | Ctrl+Shift+Y | `/copy` |
| Toggle expanded thinking | Ctrl+Shift+R | `/thinking` |
| Toggle expanded tool details | Ctrl+Shift+O | `/details` |
| Split view | Ctrl+Shift+W | `/watch` |
| Toggle sidebar | Ctrl+Shift+B | `/sidebar` |
| Zoom the focused tile (split view) | Ctrl+Shift+Z | |
| Keybindings and commands | Ctrl+Shift+H | `/help` |
| Settings | Ctrl+Shift+S | |
| Move the tile focus (split view) | Alt+arrow keys | |

*Set or cycle the model variant* without an argument switches to the next variant. The help dialog (Ctrl+Shift+H) lists the same keys.

## Settings

Open them with Ctrl+Shift+S or the settings button at the bottom of the sidebar.

- **Theme:** System, Light or Dark.
- **Language:** System, English or 简体中文. The conversation area and the dialogs are translated.
- **Notifications:** the master switch and the three kinds described [above](#notifications).

They are saved in `gui.json` in the state directory ([Files and directories](cli.md#files-and-directories)), together with whether the sidebar is shown and the size of the window:

```json
{
  "theme": "system",
  "language": "system",
  "notifications": { "enabled": true, "permission": true, "turnDone": true, "turnError": true },
  "sidebar": { "visible": true },
  "window": { "width": 980, "height": 760 }
}
```

`theme` is `system`, `light` or `dark`; `language` is `system`, `en` or `zh_CN`. The file is written on every change. If it cannot be read, it is renamed to `gui.json.bad`, the defaults are used, and the settings dialog and a message in the corner say so. Values that are wrong are ignored one by one and keep their defaults.

## Known limits

- Notifications on Windows and macOS have not been tried on those systems yet.
- On Wayland, whether a click on a notification brings the window to the front depends on the desktop: without an activation token the taskbar entry may only flash.
- Ctrl+Shift+U can be taken by the IBus Unicode input on Linux. If the key does not respond, turn that IBus key off, or use the *Undo* button under your message or `/undo`.
- In very long sessions (more than about 200 messages above or below the view) the scroll bar length is an estimate, and may shift a little while you drag it.
- A bash call that exits with a non-zero code still shows as done in the card header; the exit code is in the open card.
- For tools the GUI has no special view for, the one-line summary lists the first three plain arguments in alphabetical order, where the terminal UI uses the order in the call.
- Tool arguments and output are shown as UTF-8 text.
- There is no `/editor` and no `/exit` command, and the GUI does not quit when the last session closes.

## Files and backend

The GUI uses the same instance files, configuration and session storage as `ref` ([Files and directories](cli.md#files-and-directories)), and the same environment variables.

## Building and developing

See [The GUI](development.md#the-gui) in the development guide.
