**English** · [简体中文](web.zh-CN.md)

# Web UI

- [Starting](#starting)
- [Signing in](#signing-in)
- [Finding a running instance](#finding-a-running-instance)
- [Opening it to the network](#opening-it-to-the-network)
- [Using the page](#using-the-page)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Notifications](#notifications)
- [Settings](#settings)
- [Connection](#connection)
- [Known limits](#known-limits)
- [Developing the page](#developing-the-page)

`ref-web` shows ref in a browser. It is a small web server: it serves the page, signs the browser in with a key, decrypts the browser's WebSocket frames and forwards them unchanged to `ref-server` (and the other way round). Sessions, tools and permissions all stay in the backend, so a session can be open in the terminal and in a browser tab at the same time. Like `ref`, `ref-web` finds a running backend through the instance files and otherwise starts the `ref-server` next to it; a backend started this way exits when the last client disconnects.

## Starting

```bash
ref-web                       # serve on 127.0.0.1 (random port) and open the browser
ref-web ~/src/project         # new sessions start in this directory
ref-web --port 8080 --no-open # a fixed port, do not open the browser
```

Without `--port`, `ref-web` picks a random free port in 10000–65535. It keeps the port in `<state dir>/web/port` and tries it first the next time, so the address stays the same while that port is free; when it is taken, a new one is picked and kept. `--port N` uses that port, or stops with an error if another program has it.

All options are listed in [Command line](cli.md#ref-web). Opening the page without `?session=<id>` shows the empty workspace (*No session open*) rather than creating a blank session automatically; the directory you pass sets the default directory for new sessions (without one, the directory `ref-web` was started in). The address `ref-web` prints carries it as `?dir=`; the address bar can also hold `?session=<id>` (open that session; if attaching fails, an error toast is shown without creating a session) and `?view=split` (the split view), and follows the session you switch to.

## Signing in

On start, `ref-web` prints an address that holds the key and opens it in the browser:

```
http://127.0.0.1:<port>/?dir=/home/you/src/project#key=…
```

The key is in the part after `#` (the fragment). A browser never sends the fragment to a server, so the key is not in any request, `Referer` header or log. The page reads it, removes it from the address bar and signs in. There is no cookie and no login route.

**The key.** Without `--key`, the key is the login token: 64 hexadecimal characters, created on first start and kept in `<state dir>/web/login-token` (mode 600; on Unix a wider mode is corrected with a notice). If the file holds anything else, `ref-web` stops with *does not hold a valid login token*. `--key KEY`, or the environment variable `REF_WEB_KEY`, sets the key for one run instead. It needs at least 16 characters and is not written to disk; the token file is then neither read nor created. A key on the command line shows up in the process list and the shell history, `REF_WEB_KEY` keeps it out of the process list. The default key is random and long, so prefer it.

**How the page signs in.** The key itself never travels. When the page opens the WebSocket, `ref-web` sends a fresh random number and a salt. The page answers with its own random number and a proof made from the key, and both sides derive the keys of this one connection from the key and the two numbers. From then on every message in both directions is encrypted and authenticated (XChaCha20-Poly1305). A recorded connection does not sign in again, because the next connection uses new numbers. The salt is 16 random bytes in `<state dir>/web/salt` (mode 600); it is not secret.

**Address forms.** The page takes the key from three forms of address:

| Form | Sent to the server? | Use |
|---|---|---|
| `#key=K` | No | what `ref-web` prints and opens |
| `?key=K` | Yes, in the request line | links that need the key in the query |
| `?token=K` | Yes | the old name; old `/login?token=…&dir=…` bookmarks also still work, they load the page |

A key in the query string can be read in clear by anyone on the path when the page is served over plain `http://` on a network, by a reverse proxy and its access log (nginx logs the query string by default), and from the browser history until the page removes it. When the key came in the query of a plain `http://` page on another machine, the page shows a notice saying so. Every response has `Referrer-Policy: no-referrer`.

**What the browser keeps.** The page does not store the key. It stores the key stretched with scrypt, together with the salt, in `localStorage` under `ref.web.key`, for this address only (scheme, host and port). With it, each reload and reconnect signs in without asking. Anyone who can read the browser profile, or a script running in the page, can sign in with that value until the key changes. The page's Content-Security-Policy runs only the page's own script files, and model output is shown as text. Without `localStorage` (some private modes) the page keeps the value in memory and asks again after a reload.

**The key-entry page.** The page shows *Enter the key* with a password field and *Sign in* when it has no key, when its stored key was made with another salt, or when `ref-web` refuses the key. *Checking the key…* shows while the page stretches the key (up to a second on a phone). A wrong key gives *The key was not accepted.* The page does not reconnect by itself while it needs a key.

**Restarting and changing the key.** A restarted `ref-web` keeps its key and salt, so open pages reconnect and stay signed in. To change the key, stop `ref-web`, delete `<state dir>/web/login-token` and start it again; with `--key`, start it with another key. Every browser then shows the key-entry page, and the new address `ref-web` prints signs in again. Deleting `<state dir>/web/salt` also makes every browser ask for the key again.

## Finding a running instance

Each running `ref-web` writes a record to `<state dir>/web/instances/<pid>.json` (pid, port, host, tls, version and start time; no secret; mode 600) and removes it when it stops. Starting `ref-web` again looks at these records, newest first. If one of them belongs to a `ref-web` with the same key, it prints that instance's address (with the new `DIR` and the key), and for a TLS instance its `certificate SHA-256` fingerprint first, opens the browser and exits with code 0. With `--port N`, only the instance on that port counts. The running instance keeps its own `--server`, `--mock`, `--host` and `--no-tls`; the options of the second start are not applied, and a notice says so. `ref-web` recognizes its own instance by an answer to `/api/ping` that is signed with a key derived from the key (over pinned TLS when the record says `tls: true`); `/api/ping` answers only callers on the same machine. Records of processes that no longer exist are deleted.

If no instance matches, a new one starts. With `--port N`, a `ref-web` with a different key in the records stops the start with *a ref-web with a different key is running on port N (pid P); stop it, or pass the same --key*. A port taken by another program (or by a `ref-web` of another state directory) gives *port N is in use by another program or by a ref-web with a different key*. Both exit with code 1. Two starts at the same moment can both start an instance; that does no harm.

## Opening it to the network

`ref-web` listens on `127.0.0.1` only (plain `http://`, no certificate warning). `--host ADDR` listens on another address, for example `--host 0.0.0.0` for the local network, and **enables HTTPS by default** with a self-signed certificate kept in `<state dir>/web/tls/`. It prints a warning when you do.

- **First visit.** A browser does not know the self-signed certificate, so the first time you open the address (`https://…`) it warns you that the connection is not private (e.g. Chrome/Edge: *Your connection is not private* → *Advanced* → *Proceed*; Firefox: *Warning: Potential Security Risk Ahead* → *Advanced…* → *Accept the Risk and Continue*; Safari: *This Connection Is Not Private* → *Show Details* → *visit this website*). Before you continue, compare the fingerprint `ref-web` printed (`ref-web: certificate SHA-256 …`) with the one the browser shows for the certificate; if they are equal, continue. If the fingerprint differs, do not continue: someone else answers on that address. Opening the page with `http://` on the TLS port redirects (`308`) to `https://` without path or query.
- **Addresses and certificate names.** The certificate covers `localhost`, `127.0.0.1`, `::1` and `--host` (for `0.0.0.0` or `::`, the machine's primary outbound IPv4 address if available). Open the address `ref-web` prints (`https://<address>:<port>/`); other addresses show a name mismatch next to the authority warning. Other hostnames are rejected: `ref-web` checks the `Host` header against the addresses it knows to prevent DNS rebinding. The certificate renews on start within 30 days of expiry or when `--host` adds a new name.
- **`--no-tls` and reverse proxies.** `--no-tls` serves plain `http://` on a non-loopback `--host` (on loopback it is a silent no-op). The messages are still encrypted with the key, but the page and its scripts travel in plain text, so someone on the path can change the page and steal the key (see [Known limits](#known-limits)). Use `--no-tls` behind a reverse proxy that terminates TLS, or keep `ref-web` on loopback behind an SSH tunnel or nginx:

  ```bash
  # SSH tunnel: on the server run `ref-web --no-open --port 7480`
  ssh -L 7480:127.0.0.1:7480 server
  # then open http://127.0.0.1:7480/ on your machine
  ```

  ```nginx
  # nginx in front of `ref-web --no-open --port 7480` (rewrites Host and Origin to match loopback)
  location / {
      proxy_pass http://127.0.0.1:7480;
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_set_header Host 127.0.0.1:7480;
      proxy_set_header Origin http://127.0.0.1:7480;
  }
  ```

To use a backend that runs somewhere else, pass `--server ws://host:port --token T` (see [Running the backend on another machine](cli.md#running-the-backend-on-another-machine)). `--token` needs `--server`, and `--server` cannot be combined with `--mock`.
## Using the page

The page is the same interface as the [desktop GUI](gui.md). This section lists what it has and where it differs; the GUI page describes the conversation in more detail, and tool cards, permissions, images, undo and compaction look and work the same.

**The window**

- The sidebar on the left starts with the to-do list of the session on screen (in the split view, of the focused tile), "Todo done/total" with the items below it; the section is absent while that session has no list, and scrolls inside once it passes a third of the sidebar. Below it come the running sessions, the history with a search box, and a footer with MCP server status, *Configuration (ref.json)*, *Reload configuration* and *Settings*. Click a *Todo*, *Running*, or *History* header to collapse or expand that section, freeing its vertical space for the remaining expanded ones. It can be hidden (Alt+B) and resized on wide windows by dragging its right border (210–480 px; double-click to reset to 264 px); visibility, width, and section fold states are remembered. In a window narrower than 800 px the sidebar becomes a drawer over the content, and there is no split view.
- Running sessions are labelled with their name, else their title, else the name of their directory. Sessions in the history are labelled with their title (on one line), else their name, else the name of their directory, as in the terminal UI. The *Running* list is as tall as its rows and scrolls only when *History* would get fewer than three rows.
- *New session* creates a session in the page's default directory. To start one in another directory, use the folder button next to it, *New session in folder…* in the empty workspace, or the command palette; choosing a folder also updates the workspace's default directory for later sessions. It asks for a path; Tab or a click completes sub-directories, which the backend lists.
- The command palette (Ctrl+P, Cmd+P on macOS) runs any command. Commands without an argument run at once; choosing rename, change directory, export, variant or close live sessions opens its dedicated dialog directly, and pressing Esc while entering custom-command arguments returns to the command list. The `/` list in the input box shows the same commands, custom commands included. `/config` (`/configuration`, the sidebar footer button, or *Settings* → *Edit ref.json…*) opens the 860 px six-section visual and raw JSONC editor for global `ref.json` with multi-profile switching and Ctrl+S (Cmd+S) save; `/reload` (`/reload-config`) hot-reloads configuration across live sessions and MCP servers (see [Configuration](configuration.md#graphical-editor-and-profiles)).
- The sessions dialog (Alt+L or `/sessions`) filters between *All*, *User-created*, and *By creator* of the selected row, and searches creator names and ids alongside session names and directories.
- Clicking the already-open session in *Running* (in single view), choosing *Move to background* from its context menu, or running `/detach` (`/background`, `/leave`) moves the session to the background without stopping it and returns to *No session open*. Closing a working session asks for confirmation first; closing the last open session returns to *No session open* instead of starting a new one (and in split view, closing the held foreground session clears it without pulling a background tile out of the grid). There is no `/editor` and no `/exit`. `/compact` asks for no confirmation.
- When the backend is older than the page, a banner at the top offers *Restart backend*. The button is disabled while a session is running or waiting for permission. The restart stops the backend, the page reconnects, and `ref-web` starts the installed, newer one.
- Errors from sessions you are not looking at (and from any session in the split view) appear as a message in the top right corner, "name: first line of the error". At most four show at once, for 3.5 s (info), 6 s (warning) or 8 s (error); hovering pauses the timer, and a repeated message restarts the timer instead of stacking. The error of the session on screen is shown at the end of its conversation.

**Split view**

- Alt+W or the button at the top right shows one tile per live background session (no other client has open; the current foreground session stays hidden while split view is on and returns when you leave). Split view starts with no tile focused: click a tile or press Alt+arrow keys to focus it; click the focused tile again or the empty grid background to deselect. Commands act on the focused tile. Zoom a tile with Alt+Z or its button; the button then reads *Back to the split view*. Tiles spawned by another session show a `⟲ <root>` chip in the header and tint unfocused borders and status dots by their root task; user-created tiles stay unmarked.
- Only the focused tile shows its input box. The other tiles fold it away, with the chips and the attachment strip, and keep the status line (state, elapsed time, context meter) and the queue of waiting messages. Draft text, attachments, shell mode and the cursor are kept, and the box unfolds with the cursor in it when the tile gets the focus. A zoomed tile shows its input box as usual.

**The conversation**

- Alt+R opens or folds all reasoning blocks, Alt+O all tool details; clicking a block's header changes only that block. The to-do list of a `todo` call cannot be folded. A card that waits for permission is always open (for `edit` and `write`, it shows a *Preview* diff above the buttons, computed when the request was asked).
- `edit` and `write` cards show the path, `+`/`-` counts, the first 8 diff rows (with `+N more`), and any edit warnings even when collapsed; expanding shows raw Input, the full diff, and Output. Finished `question` cards show a compact summary of the chosen answers when collapsed. Output of bash, read and grep shows 200 lines at most, with *Show N more lines*. A bash call that is still running shows its last 20 lines. Wide cards (560 px and more) also show the time a call took.
- Under a message: who wrote it, the time and the tokens; *Copy* (the message text) and, on your own messages, *Undo*. Alt+Y copies the last reply, Alt+U undoes the last prompt and puts it back in the input box.
- When you scroll near the top (about 400 px), older messages load by themselves without moving the view; *Load older messages* does the same on a click.
- While the session has a to-do list, its status line (also in every split tile) shows the item being worked on, else the first pending one, and *done/total*, as the terminal UI does; cancelled items count in neither number. When everything is done it shows a tick and *done/total* in green. A click on it opens the whole list in a popover (Esc, a click elsewhere or another session closes it); the list updates while it is open.
- Permission keys: with a request waiting on the focused session (in the split view, on the focused or zoomed tile only), a or Enter allows once, s allows for the session, d or Esc denies. The keys wait 350 ms after a request appears, and again when the focus moves to a tile that has one. After that, other letter keys are not typed into the input box until the request is answered. Open dialogs keep their keys. A request in another tile is answered with the buttons of its card, or after you focus that tile.
- Images are shown as thumbnails; a click opens the picture large, Esc closes it. Links open only for http, https and mailto; images in the model's Markdown are shown as links and HTML as text, so a conversation causes no requests by itself.

**Questions from the model**

When the model calls the `question` tool, the questions appear inside the card of that call, which opens for them. The card, the session's row in the sidebar and the status line say *Waiting for your answer* until you answer, and the session's tile header is highlighted as it is for a permission request.

- Each question has a short heading, the question and its options, each with a label and a description. The option the model recommends is preselected and marked *(default)*. A question with one choice and no other question answers at once: a click, a number key 1 to 9, or Enter on the highlighted row (the cursor starts on the default).
- With several questions, or a question that allows more than one choice, there is a tab per question and a last tab, *Confirm*. Picking in a single choice moves on to the next tab; a multiple choice has checkboxes, with the recommended options checked, and → or Tab goes on to the next tab. *Confirm* lists the answers and marks a question you left alone as *Unanswered*. *Submit* or Enter sends them; an unanswered question is sent as unanswered.
- When the model allows it, the last row is *Type your own answer*. Choosing it opens a text field: Enter accepts the text (an empty text does not count), Esc leaves the field. In a multiple choice the text is added to the checked options.
- *Dismiss* or Esc dismisses the questions; the model is told that you did not answer.
- Keys, as in the terminal UI and the GUI: ↑ and ↓ or k and j move the cursor (it stops at the first and last row), 1 to 9, Space or Enter pick the row, or flip its checkbox in a multiple choice, ← and →, Tab and Shift+Tab change the tab (when there are tabs), Enter on the *Confirm* tab submits, Esc dismisses. The keys follow the rules of the permission keys: they wait 350 ms, other letters are not typed into the input box while a question waits, the text field keeps its own keys, and in the split view they act only on the focused or zoomed tile. A question in another tile is answered with the buttons and rows of its card, or after you focus that tile.
- When the question was answered in another window first, its card goes away without a message. An answer the backend refuses shows its message in the top right corner and the card stays.

**The input box**

| Key | Action |
|---|---|
| Enter | Send (or in `/cd`, `/export` and `!` shell mode, run the typed text unless a completion was explicitly selected with ↑/↓). On a standalone `/` completion list, Enter executes the selected command directly. While the session is busy the prompt waits in a queue; *Remove* takes it back. |
| Shift+Enter, Ctrl+J | New line. |
| Tab, Shift+Tab | Next or previous agent (outside shell mode). In `/`, `@` and `!` shell mode, Tab completes (without adding a trailing space on standalone slash commands, and drilling into subdirectories on paths). |
| Alt+T | Next model variant (Ctrl+T belongs to the browser). |
| Esc, Esc | Interrupt the running turn (the second press within 1.5 seconds). A first Esc closes an open list, leaves shell mode (keeping any typed text), or, when a tool call waits for its delay, stops the turn before the tool runs. |
| ↑, ↓ | Prompt history (filtered to `!` entries in shell mode): ↑ from an empty box or from the start of the text, and while browsing from the first line; ↓ from the last line while browsing. |
| Ctrl+C | Without a selection, empties the input box; with a selection it copies. |
| `!` at the start | Shell mode: the line runs as a shell command, with `!` history + `$PATH` binary completion for commands (including after `VAR=val` assignments and command wrappers such as `sudo`, `env`, `nice`, and `timeout`) and file/directory completion for arguments. Backspace on an empty box or Esc leaves it. |

- Paste an image or drop any file on the box (or paste `file://` URLs): supported images (PNG, JPEG, GIF or WebP, up to 20 MiB each) attach as `[Image N]` tokens, as in the terminal UI; other files insert their path at the caret (or filename when the browser withholds the path).
- `/` triggers command completion both at the start of the box and mid-text after whitespace, matches commands case-insensitively (including singular aliases `/model` and `/agent`, `/models <m>`, `/agents <a>`, `/variant <v|default>`, `/sessions <s>`, `/close-session <s…>`), and completes arguments for `/cd`, `/export`, `/models`, `/agents`, `/variant`, `/sessions` and `/close-session` (plus `@file` inside custom command arguments). Standalone commands (all built-ins except `/rename`) complete without a trailing space on Tab and run on Enter; unknown single-word `/foo` inputs are blocked with a warning toast and fuzzy suggestion instead of being sent as a prompt. `@` lists files under the session's directory and inserts `@path `.

## Keyboard shortcuts

The shortcuts are Alt+letter, matched by the physical key, so they work with any keyboard layout. Browsers keep Ctrl+N, Ctrl+T and Ctrl+W for themselves, which is why the page does not use them. The letters are the same in the [GUI](gui.md#keyboard-shortcuts), which uses Ctrl+Shift instead of Alt.

| Command | Shortcut | `/` command |
|---|---|---|
| Command palette | Ctrl+P (Cmd+P on macOS) | |
| New session (same directory) | Alt+N | `/new` |
| Switch session (live and history) | Alt+L | `/sessions` |
| Switch model (this session only) | Alt+M | `/models` (`/model`) |
| Switch agent | Alt+A | `/agents` (`/agent`) |
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

The page can show a system notification when a session you are not looking at needs a tool permission, asks a question, finishes a turn, or ends a turn with an error. The session on screen never notifies. A click on the notification opens the session. The text is *Waiting for permission to use {tool}*, *Asks: {question}* (the first question), *Finished* or *Stopped with an error: {error}*.

- The browser has to allow it. A bar at the top of the page asks once (*Allow* or *Not now*; the answer to *Not now* is remembered in `localStorage`), and the settings have the same button.
- Browsers show notifications only on `localhost`, `127.0.0.1` and `https` pages. On a plain `http` address on the network, and when the browser blocks notifications, only the tab title and the tab icon show that something needs attention.
- The settings have a master switch and three kinds: a session waits for you (permission or question), a turn finished, a turn ended with an error. They are all on by default.

## Settings

Alt+S or the settings button in the sidebar opens them: the theme (System, Light or Dark), the language (System, English or 简体中文), the notifications, and *Configuration* (*Reload configuration* and *Edit ref.json…*, see [Configuration](configuration.md#graphical-editor-and-profiles)). Preferences are saved in the browser (`localStorage`, key `ref.settings`, together with sidebar visibility, width, and section fold states) and are shared between the tabs of that browser. A wrong value in the stored settings is replaced by its default; the others are kept. All UI transitions honor `prefers-reduced-motion: reduce`.

## Connection

- `ref-web` pings the WebSocket of every tab every 30 seconds. When a tab sends nothing at all (no answer to the ping and no other frame) for 60 seconds, or `ref-web` cannot send it a frame for 60 seconds because the browser does not read, the connection of that tab and its connection to the backend are closed. A laptop that sleeps or loses its network therefore leaves no stale session subscriptions in the backend.
- When the connection is lost the page reconnects by itself and signs in again with the stored key. When `ref-web` refuses the key (close code 4401, for example after the key was changed), the page stops retrying and shows the key-entry page (see [Signing in](#signing-in)). While `ref-web` is not running, the page keeps trying.
- Other close codes of `ref-web`: 4400 for a malformed sign-in or a message that does not decrypt, 4408 when the sign-in does not finish within 10 seconds, 1011 *backend unavailable* when the backend cannot be reached (the details are in `ref-web`'s log).

## Known limits

- Notifications need `localhost` or `https` (see above).
- Some browsers use Alt+←/→ for back and forward. Moving the tile focus with Alt+arrow keys has not been tried in every browser.
- Alt+letter keys can produce special characters on some keyboard layouts; the page matches the physical key and ignores the character.
- The prompt history and the settings are per browser and are not shared with the terminal UI or the GUI.
- Tool arguments and output are shown as UTF-8 text.

What the key and its encryption do not protect against:

- **Someone who can change the page.** Over plain `http://` on a network (`--no-tls`, or an upstream leg without TLS), someone on the path can serve a modified page whose script reads the key as you type it, or the stored value. The encryption cannot prevent this, because the code that encrypts comes over the same connection. With `ref-web`'s own self-signed TLS, this holds unless you compared the SHA-256 fingerprint on the first visit (trust on first use). On a network you do not trust, use an SSH tunnel or a reverse proxy with a real certificate.
- **Traffic analysis.** The sizes and timing of messages are visible, and without TLS also the path, the `Host` header and the fact that `ref-web` is in use.
- **The endpoints.** Anyone who can read `ref-web`'s state directory or the browser's profile has the key. A key given with `--key` is visible in the process list and the shell history on that machine.
- **The backend leg.** `ref-web` talks to `ref-server` over plain `ws://`. On one machine that stays on loopback; with `--server ws://remote` it crosses the network.
- **A key typed into someone else's page.**

## Developing the page

The page is in `web/` (Vue 3, TypeScript, Vite, pnpm). `ref-web` embeds the built files, so see [Development](development.md#building) for how to build. Run `pnpm install && pnpm build` in `web/` before building `ref-web` or running `cargo build`, `cargo test` or `cargo clippy` with `--workspace`; otherwise `build.rs` stops with an error.

```bash
cd web
pnpm install
pnpm dev                  # Vite dev server on 127.0.0.1:5173
```

`pnpm dev` forwards `/ws` and `/api` to `ref-web` on `127.0.0.1:7480` (`REF_WEB_URL` sets another address, including `https://…` against a TLS `ref-web` — the proxy accepts its self-signed certificate). Start `ref-web --no-open --port 7480` (or `ref-web --mock --no-open --port 7480`) first, then open `http://127.0.0.1:5173/#key=…` with the key from the address `ref-web` prints. The page keeps the key for the dev server's address. Use `127.0.0.1` throughout, not `localhost`: the stored key belongs to one address. A dev build that talks to a `ref-server` directly (`VITE_REF_URL`) uses no key and no encryption.
