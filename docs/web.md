**English** · [简体中文](web.zh-CN.md)

# Web UI

- [Starting](#starting)
- [Signing in](#signing-in)
- [One instance per port](#one-instance-per-port)
- [Opening it to the network](#opening-it-to-the-network)
- [Using the page](#using-the-page)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Notifications](#notifications)
- [Settings](#settings)
- [Connection](#connection)
- [Known limits](#known-limits)
- [Developing the page](#developing-the-page)

`ref-web` shows ref in a browser. It is a small web server: it serves the page, signs the browser in, and forwards the browser's WebSocket frames unchanged to `ref-server`. Sessions, tools and permissions all stay in the backend, so a session can be open in the terminal and in a browser tab at the same time. Like `ref`, `ref-web` finds a running backend through the instance files and otherwise starts the `ref-server` next to it; a backend started this way exits when the last client disconnects.

## Starting

```bash
ref-web                       # serve on 127.0.0.1:7480 and open the browser
ref-web ~/src/project         # new sessions start in this directory
ref-web --port 8080 --no-open # another port, do not open the browser
```

All options are listed in [Command line](cli.md#ref-web). The page uses the directory you pass as the directory for new sessions; without one, it uses the directory `ref-web` was started in. The login address carries it as `?dir=`; the address bar can also hold `?session=<id>` (open that session) and `?view=split` (the split view), and follows the session you switch to.

## Signing in

On start, `ref-web` prints a login address and opens it in the browser:

```
http://127.0.0.1:7480/login?token=…&dir=/home/you/src/project
```

The address sets a cookie and redirects to the page, so the token does not stay in the address bar or the browser history. After that, `http://127.0.0.1:7480/` works without the token until the cookie expires.

- The cookie is named `ref_web_<port>`, so two `ref-web` instances on one host do not overwrite each other. It is `HttpOnly` and `SameSite=Strict` and valid for 30 days.
- Its value is a random session id that lives in the memory of the `ref-web` process. It is not the login token: browsers send a cookie to every port of the same host, so a cookie that held the token could be read by other services on `localhost`.
- After `ref-web` restarts, the old session ids are gone. An open page then gets 401, says *Not logged in. Run ref-web again: it opens the login URL again.* and stops reconnecting. Run `ref-web` again (it opens the login address) and the page works again.

The login token is created on first start and kept in `<state dir>/web/login-token` (64 hexadecimal characters, mode 600; on Unix a wider mode is corrected with a notice), so the login address stays the same after a restart. If the file holds anything else, `ref-web` stops with *does not hold a valid login token*. To get a new token, stop `ref-web`, delete that file and start it again. Browsers signed in with the old token are signed out.

## One instance per port

Only one `ref-web` runs per port. Starting it again while one is running on that port prints the login address (with the new `DIR`) and opens the browser, then exits with code 0. The running instance keeps its own `--server`, `--mock` and `--host`; the options of the second start are not applied. `ref-web` recognizes its own instance by an answer to `/api/ping` that is signed with the login token. If another program uses the port, `ref-web` reports *port N is in use by another program* and exits with code 1.

## Opening it to the network

`ref-web` listens on `127.0.0.1` only. `--host ADDR` listens on another address, for example `--host 0.0.0.0` for the local network. It prints a warning when you do.

- Open the page by **IP address**, for example `http://192.168.1.20:7480/`. A host name is rejected: `ref-web` checks the `Host` header against the addresses it knows to prevent DNS rebinding.
- There is no TLS. The cookie and everything you type travel in plain text, and whoever signs in controls the backend, which can run commands on that machine. Use an SSH tunnel or a reverse proxy with TLS instead:

  ```bash
  ssh -L 7480:127.0.0.1:7480 server
  # then open http://127.0.0.1:7480/ on your machine
  ```

To use a backend that runs somewhere else, pass `--server ws://host:port --token T` (see [Running the backend on another machine](cli.md#running-the-backend-on-another-machine)). `--token` needs `--server`, and `--server` cannot be combined with `--mock`.

## Using the page

The page is the same interface as the [desktop GUI](gui.md). This section lists what it has and where it differs; the GUI page describes the conversation in more detail, and tool cards, permissions, images, undo and compaction look and work the same.

**The window**

- The sidebar on the left lists the running sessions, the history with a search box, the state of the MCP servers and the settings. It can be hidden (Alt+B); the state is remembered. In a window narrower than 800 px the sidebar becomes a drawer over the content, and there is no split view.
- Running sessions are labelled with their name, else their title, else the name of their directory. Sessions in the history are labelled with their title (on one line), else their name, else the name of their directory, as in the terminal UI. The *Running* list is as tall as its rows and scrolls only when *History* would get fewer than three rows.
- *New session* creates a session in the directory of the page. To start one in another directory, use the folder button next to it or the command *New session in another folder…* (no shortcut). It asks for a path; Tab or a click completes sub-directories, which the backend lists.
- The command palette (Ctrl+P, Cmd+P on macOS) runs any command. Commands without an argument run at once; rename, change directory, export, variant, close a session and the peer name ask for it first. The `/` list in the input box shows the same commands, custom commands included.
- Closing the last session starts a new one in the same directory; the page never ends up empty. There is no `/editor` and no `/exit`. `/compact` asks for no confirmation.
- When the backend is older than the page, a banner at the top offers *Restart backend*. The button is disabled while a session is running or waiting for permission. The restart stops the backend, the page reconnects, and `ref-web` starts the installed, newer one.
- Errors from sessions you are not looking at (and from any session in the split view) appear as a message in the top right corner, "name: first line of the error". At most four show at once, for 3.5 s (info), 6 s (warning) or 8 s (error); hovering pauses the timer, and a repeated message restarts the timer instead of stacking. The error of the session on screen is shown at the end of its conversation.

**Split view**

- Alt+W or the button at the top right shows one tile per background session: the live sessions that no other client has open, plus the current session. Click a tile, or press Alt+arrow keys, to move the focus; commands act on the focused tile. Zoom a tile with Alt+Z or its button; the button then reads *Back to the split view*.

**The conversation**

- Alt+R opens or folds all reasoning blocks, Alt+O all tool details; clicking a block's header changes only that block. The to-do list of a `todo` call cannot be folded. A card that waits for permission is always open.
- Output of bash, read and grep shows 200 lines at most, with *Show N more lines*. A bash call that is still running shows its last 20 lines. Wide cards (560 px and more) also show the time a call took.
- Under a message: who wrote it, the time and the tokens; *Copy* (the message text) and, on your own messages, *Undo*. Alt+Y copies the last reply, Alt+U undoes the last prompt and puts it back in the input box.
- When you scroll near the top (about 400 px), older messages load by themselves without moving the view; *Load older messages* does the same on a click.
- Permission keys: with a request waiting on the focused session (in the split view, on any tile), a or Enter allows once, s allows for the session, d or Esc denies. The keys wait 350 ms after a request appears. After that, other letter keys are not typed into the input box until the request is answered. Open dialogs keep their keys.
- Images are shown as thumbnails; a click opens the picture large, Esc closes it. Links open only for http, https and mailto; images in the model's Markdown are shown as links and HTML as text, so a conversation causes no requests by itself.

**The input box**

| Key | Action |
|---|---|
| Enter | Send. While the session is busy the prompt waits in a queue; *Remove* takes it back. |
| Shift+Enter, Ctrl+J | New line. |
| Tab, Shift+Tab | Next or previous agent. With the `/` or `@` list open, Tab completes. |
| Alt+T | Next model variant (Ctrl+T belongs to the browser). |
| Esc, Esc | Interrupt the running turn (the second press within 1.5 seconds). A first Esc closes an open list, leaves shell mode, or, when a tool call waits for its delay, stops the turn before the tool runs. |
| ↑, ↓ | Prompt history: ↑ from an empty box or from the start of the text, and while browsing from the first line; ↓ from the last line while browsing. |
| Ctrl+C | Without a selection, empties the input box; with a selection it copies. |
| `!` at the start | Shell mode: the line runs as a shell command. Backspace on an empty box or Esc leaves it. |

- Paste an image from the clipboard or drop image files on the box (PNG, JPEG, GIF or WebP, up to 20 MiB each). The text gets an `[Image N]` token, as in the terminal UI. A browser cannot paste the path or `file://` address of a file.
- `@` lists files under the session's directory and inserts `@path `.
- The prompt history (500 entries) is kept in the browser (`localStorage`, key `ref.promptHistory`). It is not shared with the terminal UI or the GUI.

## Keyboard shortcuts

The shortcuts are Alt+letter, matched by the physical key, so they work with any keyboard layout. Browsers keep Ctrl+N, Ctrl+T and Ctrl+W for themselves, which is why the page does not use them. The letters are the same in the [GUI](gui.md#keyboard-shortcuts), which uses Ctrl+Shift instead of Alt.

| Command | Shortcut | `/` command |
|---|---|---|
| Command palette | Ctrl+P (Cmd+P on macOS) | |
| New session (same directory) | Alt+N | `/new` |
| Switch session (live and history) | Alt+L | `/sessions` |
| Switch model (this session only) | Alt+M | `/models` |
| Switch agent | Alt+A | `/agents` |
| Set or cycle the model variant | Alt+T | `/variant` |
| Compact the conversation (no confirmation) | Alt+C | `/compact` |
| Undo last prompt | Alt+U | `/undo` |
| Close this session | Alt+K | `/close` |
| Export transcript as Markdown | Alt+X | `/export` |
| Copy last assistant message (raw text) | Alt+Y | `/copy` |
| Toggle expanded thinking | Alt+R | `/thinking` |
| Toggle expanded tool details | Alt+O | `/details` |
| Split view | Alt+W | `/watch` |
| Toggle sidebar | Alt+B | `/sidebar` |
| Zoom the focused tile (split view) | Alt+Z | |
| Keybindings and commands | Alt+H | `/help` |
| Settings | Alt+S | |
| Move the tile focus (split view) | Alt+arrow keys | |

*Set or cycle the model variant* without an argument switches to the next variant. The help dialog (Alt+H) is generated from the command list, so it always shows the keys that work. While a dialog is open it keeps the keyboard; only Ctrl+P still opens and closes the palette. On macOS, Alt+arrow keys inside the input box still move the cursor by words.

## Notifications

The page can show a system notification when a session you are not looking at needs a tool permission, finishes a turn, or ends a turn with an error. The session on screen never notifies. A click on the notification opens the session. The text is *Waiting for permission to use {tool}*, *Finished* or *Stopped with an error: {error}*.

- The browser has to allow it. A bar at the top of the page asks once (*Allow* or *Not now*; the answer to *Not now* is remembered in `localStorage`), and the settings have the same button.
- Browsers show notifications only on `localhost`, `127.0.0.1` and `https` pages. On a plain `http` address on the network, and when the browser blocks notifications, only the tab title and the tab icon show that something needs attention.
- The settings have a master switch and three kinds: a tool needs permission, a turn finished, a turn ended with an error. They are all on by default.

## Settings

Alt+S or the settings button in the sidebar opens them: the theme (System, Light or Dark), the language (System, English or 简体中文) and the notifications. They are saved in the browser (`localStorage`, key `ref.settings`, together with whether the sidebar is shown) and are shared between the tabs of that browser. A wrong value in the stored settings is replaced by its default; the others are kept.

## Connection

- `ref-web` pings the WebSocket of every tab every 30 seconds. When a tab sends nothing at all (no answer to the ping and no other frame) for 60 seconds, or `ref-web` cannot send it a frame for 60 seconds because the browser does not read, the connection of that tab and its connection to the backend are closed. A laptop that sleeps or loses its network therefore leaves no stale session subscriptions in the backend.
- When the connection is lost the page reconnects by itself. If `ref-web` was restarted, the page asks `/api/info` first; a 401 means the cookie is stale, so the page shows *Not logged in…* instead of retrying (see [Signing in](#signing-in)). While `ref-web` is not running, the page keeps trying.

## Known limits

- Notifications need `localhost` or `https` (see above).
- Some browsers use Alt+←/→ for back and forward. Moving the tile focus with Alt+arrow keys has not been tried in every browser.
- Alt+letter keys can produce special characters on some keyboard layouts; the page matches the physical key and ignores the character.
- The prompt history and the settings are per browser and are not shared with the terminal UI or the GUI.
- Tool arguments and output are shown as UTF-8 text.

## Developing the page

The page is in `web/` (Vue 3, TypeScript, Vite, pnpm). `ref-web` embeds the built files, so see [Development](development.md#building) for how to build. Run `pnpm install && pnpm build` in `web/` before building `ref-web` or running `cargo build`, `cargo test` or `cargo clippy` with `--workspace`; otherwise `build.rs` stops with an error.

```bash
cd web
pnpm install
pnpm dev                  # Vite dev server on 127.0.0.1:5173
```

`pnpm dev` forwards `/ws`, `/api` and `/login` to `ref-web` on `127.0.0.1:7480`. Start `ref-web --no-open` (or `ref-web --mock --no-open`) first, then sign in through the dev server with `http://127.0.0.1:5173/login?token=…`, using the token from the login address. Use `127.0.0.1` throughout, not `localhost`, because the cookie belongs to one host name.
