# Storyboard

One trailer that shows every headline feature fast, and one short film per feature. Everything is 1920×1080, 30 fps, H.264, no audio track (see [Decisions](#decisions)).

| Id | Film | Length |
|---|---|---|
| `trailer` | The trailer: all features, one beat each | 60 s |
| `backend` | One backend, every window | 28 s |
| `sessions` | Many sessions, one screen | 30 s |
| `peers` | Sessions that talk to each other | 28 s |
| `permissions` | You decide what runs | 28 s |
| `edits` | Edits that cannot go stale | 24 s |
| `composer` | A prompt box with shortcuts | 30 s |
| `long-sessions` | Sessions that do not run out | 26 s |
| `setup` | Your config, any model | 24 s |
| `notify` | Walk away, get called back | 22 s |

Why these nine: each is something a reader cannot get from "a coding agent" alone, each fits one story in under 30 seconds, and each can be shown with the scripted mock backend. Not filmed on their own: the Windows/macOS/Linux builds and the install steps (a README table does that better), and the key bindings (they appear inside the films that use them).

## Decisions

- **Languages.** Each film is rendered twice, English and Chinese (`out/<id>.en.mp4`, `out/<id>.zh.mp4`), with one language per render, not two lines stacked. Stacked subtitles take twice the screen and read worse in both languages. The README and the product are bilingual, so both audiences are real; the copy is a few short lines per film, so translating costs little. The screenshots follow the language: the GUI and the web page have a Chinese interface, so Chinese renders use Chinese screenshots. The TUI has no translation, so its shots stay English in both.
- **Frame rate: 30 fps.** The product's own motion (drop-in, fades, counters) is smooth at 30 fps and the UI is mostly static between moves. 60 fps doubles render time (about 15 frames per second on this machine, so the whole set would take over an hour) for little visible gain. `--fps 60` works if wanted.
- **No audio.** Videos autoplay muted on GitHub and social feeds, so captions carry the story. A music bed can be muxed in later without touching the scenes (a licensed track is a decision for the project owner).
- **Look.** Dark theme from the product (`Theme.qml` colours through the generated `theme.css`), product icons from `assets/icons`, product easing curves. Captions are short sentences at the bottom, at most one line.
- **Honesty.** Everything in a frame comes from a real run against `ref-server --mock`. The one exception is the desktop notification in `notify`: the operating system draws it and a headless run cannot capture it, so the scene draws a stand-in and the storyboard says so.

## How a film is built

Each scene is a list of **shots**. A shot is a stretch of video with parts on it: a screenshot window with a camera that pans and zooms (`appFrame`), a replayed terminal (`terminalView`), callouts (`highlight`), a pointer (`cursor`), typed text, captions, title cards. Shots meet through a fade or a rise. Shared shots (`intro`, `outro`) are reused by every film. The trailer reuses the feature films' key shots, cut shorter.

Source of pictures (`pnpm capture`, see [README](README.md)):

| Kind | Tool | Output |
|---|---|---|
| TUI | `ref` in a pseudo terminal against the mock backend; the output is an asciicast, replayed by xterm.js | `assets/casts/*.cast`, stills in `assets/screens/` |
| WebUI | `ref-web --mock` in headless Chromium, 1600×1000 at 2× | `assets/screens/web-*.png` |
| GUI | `ref-gui --mock` with `QT_QPA_PLATFORM=offscreen`, window grabbed from the scene graph | `assets/screens/gui-*.png` |

The ones marked README are also copied, smaller, to `docs/images/`.

## Trailer (60 s)

| Time | Picture | Caption (en) | 中文 |
|---|---|---|---|
| 0–3 | A terminal window with a running turn. The window closes. The timer in a small `ref-server` pill keeps counting. | Close the window. It keeps working. | 关掉窗口，它还在干活。 |
| 3–7 | Logo and name drop in. | A coding agent with a backend of its own. | 有独立后端的编程智能体。 |
| 7–12 | `backend`: three windows (TUI, GUI, browser) show the same session. | One backend, every window. | 一个后端，随处接入。 |
| 12–17 | `sessions`: tiles of running sessions fly into the split view; one zooms. | Many sessions at once. | 多个会话同时跑。 |
| 17–22 | `peers`: a message bubble with a sender name arrives; a helper session reports back. | Sessions message each other. | 会话之间互发消息。 |
| 22–27 | `permissions`: the permission card; `a` is pressed; the countdown ring runs out. | You decide what runs. | 能做什么，你说了算。 |
| 27–32 | `edits`: a diff block with the line tags `#56B0` highlighted. | Edits address hash-tagged lines. | 按带哈希的行号改文件。 |
| 32–37 | `composer`: `/`, `@`, `!` open their lists one after another. | `/` commands, `@` files, `!` shell. | `/` 命令，`@` 文件，`!` 命令行。 |
| 37–42 | `long-sessions`: the context meter climbs, a summary block folds shut. | Compaction and endless retries. | 自动压缩，无限重试。 |
| 42–47 | `setup`: `opencode.json` types itself; the model list fills in. | Your opencode config works as is. | 现有 opencode 配置直接可用。 |
| 47–52 | `notify`: a notification slides in; a click brings the window forward. | Called back when it needs you. | 需要你时，通知你。 |
| 52–56 | Three windows side by side: terminal, native app, browser. | Terminal. Native. Browser. | 终端、原生、浏览器。 |
| 56–60 | Logo, `github.com/GDNDZZK/ref`. | | |

## Feature films

Every film starts with a title card (2.5 s: the film's name, one line) and ends with the shared outro (2 s). The tables list what is between. "Mock" is the prompt that makes `ref-server --mock` produce the scene (`docs/development.md#the-mock-backend`).

### `backend`: One backend, every window (28 s)

Claim: `ref-server` runs the sessions; `ref`, `ref-gui` and `ref-web` only show them. README: "Server and interfaces are separate."

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–8 | TUI cast: a prompt is typed, the turn starts, tool calls stream. | Start a task in the terminal. | 在终端里开始一个任务。 | `tools`, approve with `a` |
| 8–13 | The terminal window shrinks away. A `ref-server` pill stays, its session timer counting. | Close the window. The session keeps working. | 关掉窗口，会话继续运行。 | |
| 13–19 | Browser window opens on `ref-web`; the same session, mid-turn, is in the sidebar. | Open it from a browser. | 在浏览器里接着看。 | `slow` |
| 19–24 | The native window opens; terminal, app and browser sit side by side with the same transcript. | Or the native app. Or all of them at once. | 或者原生应用，或者全部一起。 | |
| 24–26 | Pill with three arrows into one box: `ref-server`. | | | |

### `sessions`: Many sessions, one screen (30 s)

Claim: live sessions in a sidebar, a split view that tiles the background ones, zoom in and out. README: "Many sessions at once."

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–9 | Sidebar with four running sessions: status dots, todo progress `2/5`, turn timers. New sessions drop in from above. | Every session, with its progress. | 每个会话的进度一目了然。 | `todo`, `running`, `article`, `slow` |
| 9–17 | Split view: the sessions become tiles; they enter one by one, the others slide to make room. Focus moves with the arrow keys. | Watch them all at once. | 同时盯着所有会话。 | `ref -w` |
| 17–23 | One tile expands to the full conversation (shared-element zoom), then goes back. | Zoom into one. Zoom back out. | 放大其中一个，再缩回来。 | |
| 23–28 | A tile shows the amber "waiting for permission" mark; the sidebar row and the jump-to-latest button show it too. | Anything that needs you stands out. | 需要你的会话会提示。 | `permission` |

### `peers`: Sessions that talk to each other (28 s)

Claim: sessions list each other, send messages, and the `build` agent starts worker sessions. README: "Sessions can send each other messages."

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–8 | `/peers` dialog: other live sessions with status, agent, directory. | See the other sessions. | 看到其他会话。 | `peer` first |
| 8–15 | The tool call that creates the `reviewer` session `helper`; a new tile appears in the split view. | The agent can start helpers. | 智能体可以启动帮手。 | `peer` |
| 15–22 | In the main session a message bubble in another tint arrives, with the sender's name, after a turn divider. | They report back. | 它们会回报结果。 | `peer` |
| 22–26 | `/peers-outbox`: "queued → delivered → read" ticks. | You can follow every message. | 每条消息都可追踪。 | |

### `permissions`: You decide what runs (28 s)

Claim: allow, ask or deny per tool, with patterns; delays before a tool runs. README: "Control over tool calls."

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–5 | `opencode.json` `permission` block typing itself: `"edit": "ask"`, `"bash": {"curl *": "deny"}`. | Allow, ask or deny, per tool and pattern. | 按工具和模式允许、询问或拒绝。 | |
| 5–12 | The permission card unfolds under an `edit` call; the diff is shown; the pointer rests on **Allow once / Always allow / Deny** while the keys `a` `s` `d` light up. | Review the call. Press a, s or d. | 先看清调用，再按 a、s、d。 | `permission` |
| 12–19 | A `delete` call with a ring that shrinks; `Esc` is pressed and the call turns grey: cancelled. | A countdown gives you time to stop it. | 倒计时让你来得及拦下。 | `delay` |
| 19–24 | A denied `curl` call, quiet grey, and the model carries on. | Denied calls do not stop the work. | 被拒绝的调用不会中断工作。 | `denied` |

### `edits`: Edits that cannot go stale (24 s)

Claim: `read` and `grep` return `[path#TAG]` and numbered lines, `edit` takes that tag, and a changed file is detected instead of corrupted. README: "Reliable file edits." Details: `docs/tools.md#line-anchors`.

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–8 | A `read` result: `[src/parser.rs#56B0]` and `20:    for i in 0..=items.len() {`. The tag and line number are circled. | Every read comes with a tag and line numbers. | 每次读取都带标签和行号。 | `tools` |
| 8–15 | The `edit` call: input `{"tag": "56B0", "edits": [{"op": "replace", "start": 20, …}]}` next to the rendered diff, `-` red, `+` green. | An edit names the tag and the lines it changes. | 修改时指明标签和行号。 | `tools` |
| 15–21 | Split picture: the file changes on disk (tag `56B0` → `9C41`); the old edit comes back with "file changed", and the current lines are shown. | A stale view is caught, not applied. | 过期的视图会被发现，而不是被写入。 | Drawn from the doc text, not a mock run (the mock has no stale case). |

### `composer`: A prompt box with shortcuts (30 s)

Claim: `/` commands, `@` files, `!` shell, pasted images, queued prompts, undo. `docs/usage.md#writing-prompts`.

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–7 | `/` opens the command list; typing `co` narrows it to `/compact`, `/copy`. | `/` for commands. | `/` 调出命令。 | |
| 7–11 | `@pa` opens the file list: `src/parser.rs` is picked and inserted. | `@` for files. | `@` 引用文件。 | |
| 11–15 | `!ls` turns the box a different tint; the output lands in the conversation without a model turn. | `!` runs a command, no model needed. | `!` 直接执行命令，不经过模型。 | |
| 15–19 | An image is pasted; a thumbnail chip appears; the turn reads it. | Paste an image. | 直接粘贴图片。 | `image` |
| 19–24 | A prompt sent mid-turn waits above the box with "✕ remove"; the pointer takes it back and the text returns to the box. | Queue a prompt. Take it back. | 排队的提示可以撤回。 | `slow` |
| 24–28 | Undo under a message: it and everything after it disappear; the text and image return to the box. | Undo restores your draft. | 撤销会还原你的草稿。 | |

### `long-sessions`: Sessions that do not run out (26 s)

Claim: automatic compaction, retries without limit, images fitted for every provider, a steady view during long output. README: "Long sessions."

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–9 | Status line shows the context meter at 95%; the next prompt first writes a summary; the old messages fold into one block under a divider. | Full context? It compacts itself. | 上下文满了，自己压缩。 | `fill`, then any prompt |
| 9–16 | Status line: "retrying in 4 s · attempt 2" with the countdown and the provider's error; then the answer streams in. | Rate limit or outage? It retries. | 限流或故障，自动重试。 | `retry` |
| 16–22 | A tool prints 400 lines; the view follows the end; when the output collapses, the freed space stays blank until the turn ends. | The view does not jump while it works. | 工作时画面不会跳动。 | `long` |

### `setup`: Your config, any model (24 s)

Claim: an existing `opencode.json` works unchanged; Anthropic Messages, OpenAI Responses and OpenAI-compatible Chat Completions; variants; MCP over stdio and HTTP. README: "Works with opencode configuration", "Any model provider", "MCP".

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–8 | An `opencode.json` types itself: `provider`, `agent`, `mcp`. Lines get a check mark as ref picks them up. | Bring your opencode.json. | 带上你的 opencode.json。 | |
| 8–14 | The models dialog: three provider families, then `Ctrl+T` cycles `low`, `medium`, `high` on the model tag. | Any provider. Variants for effort. | 任意模型，按档位调节推理。 | |
| 14–20 | Sidebar footer and `/mcp` dialog: servers with a status dot, their tools listed. | MCP servers, status at a glance. | MCP 服务器状态一目了然。 | |

### `notify`: Walk away, get called back (22 s)

Claim: the GUI and the web page notify when a session needs permission, finishes a turn or fails, and only when you are not looking. PLAN §5 (distribution rule) and §3 (E).

| Time | Picture | Caption (en) | 中文 | Mock |
|---|---|---|---|---|
| 2.5–7 | A session is running; the window loses focus and shrinks behind another app. | Switch to something else. | 去忙别的。 | `permission` |
| 7–13 | A desktop notification slides in: "Permission required · edit". **Drawn, not captured** (the OS draws it). Browser tab: title and icon show the alert badge (captured). | It tells you when it needs you. | 需要你的时候，它会通知你。 | |
| 13–19 | Click: the window comes to the front on that session, with the permission card open. | One click, back where you were. | 点一下，回到原处。 | |
| 19–20 | Settings: the notification switches, one per kind. | Choose what you hear about. | 想听哪些，自己选。 | |

## Screens to capture (step 2, when the GUI and WebUI are finished)

`<shot>` is the file name in `assets/screens/`; GUI shots exist as `gui-<shot>` and `web-<shot>` in English and, for Chinese renders, `-zh`.

| Shot | Film(s) | State | Mock |
|---|---|---|---|
| `session-running` | backend, trailer | one session mid-turn, sidebar open | `slow` |
| `sidebar-progress` | sessions | four sessions with todo progress and timers | `todo`, `running`, `article`, `slow` |
| `split-view` | sessions, trailer | four tiles | `ref -w` |
| `split-zoom` | sessions | one tile expanded | |
| `peers-dialog` | peers | `/peers` | `peer` |
| `peer-message` | peers, trailer | peer bubble in a conversation | `peer` |
| `permission-card` | permissions, trailer | `edit` waiting, keys a/s/d | `permission` |
| `delay-ring` | permissions | countdown ring on `delete` | `delay` |
| `edit-diff` | edits, trailer | `edit` expanded with diff | `tools` |
| `composer-slash`, `composer-at`, `composer-shell`, `composer-image`, `composer-queue` | composer, trailer | each popup open | `image`, `slow` |
| `context-compact` | long-sessions, trailer | summary block under a divider | `fill` |
| `retry-status` | long-sessions | status line with countdown | `retry` |
| `models-dialog`, `mcp-dialog` | setup, trailer | dialogs open | |
| `notify-settings`, `tab-alert` | notify | settings section; tab with badge | `permission` |
| `three-windows` | backend, trailer | one session in `ref`, `ref-gui`, `ref-web` | `slow` |

TUI casts: `tui-tools` (recorded already by `pnpm capture:tui`), plus `tui-split` and `tui-peer`.

## Open questions for the project owner

- Music: none for now. If wanted, which track and licence?
- The mock workspace path (`/tmp/ref-mock-<random>/workspace`) shows in TUI and GUI status lines. The scenes crop or blur it; a fixed path would need a mock option.
