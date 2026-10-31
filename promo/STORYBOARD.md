# Storyboard

One trailer that shows every headline feature fast, one hero film (`orchestra`), and one short film per feature. Everything is 1920×1080, 30 fps, H.264, no audio track (see [Decisions](#decisions)).

| Id | Film | Length |
|---|---|---|
| `trailer` | The trailer: the crew, then one beat per feature | TRAILER_LEN s |
| `orchestra` | One session, a whole crew (the hero film) | ORCH_LEN s |
| `backend` | One backend, every window | 26.4 s |
| `sessions` | Many sessions, one screen | 27.9 s |
| `peers` | Sessions that talk to each other | 24.9 s |
| `permissions` | You decide what runs | 29.9 s |
| `edits` | Edits that cannot go stale | 25.9 s |
| `composer` | A prompt box with shortcuts | 35.4 s |
| `long-sessions` | Sessions that do not run out | 25.4 s |
| `setup` | Your config, any model | 22.9 s |
| `notify` | Walk away, get called back | 23.9 s |

Why these ten: each is something a reader cannot get from "a coding agent" alone, each fits one story in under 30 seconds, and each can be shown with the scripted mock backend. Not filmed on their own: the Windows/macOS/Linux builds and the install steps (a README table does that better), and the key bindings (they appear inside the films that use them).

## Decisions

- **Languages.** Each film is rendered twice, English and Chinese (`out/<id>.en.mp4`, `out/<id>.zh.mp4`), with one language per render, not two lines stacked. Stacked subtitles take twice the screen and read worse in both languages. The README and the product are bilingual, so both audiences are real; the copy is a few short lines per film, so translating costs little. The screenshots follow the language: the GUI and the web page have a Chinese interface, so Chinese renders use Chinese screenshots. The TUI has no translation, so its shots stay English in both.
- **Frame rate: 30 fps.** The product's own motion (drop-in, fades, counters) is smooth at 30 fps and the UI is mostly static between moves. 60 fps doubles render time (about 15 frames per second on this machine, so the whole set would take over an hour) for little visible gain. `--fps 60` works if wanted.
- **No audio.** Videos autoplay muted on GitHub and social feeds, so captions carry the story. A music bed can be muxed in later without touching the scenes (a licensed track is a decision for the project owner).
- **Look.** Dark theme from the product (`Theme.qml` colours through the generated `theme.css`), product icons from `assets/icons`, product easing curves. Captions are short sentences at the bottom, at most one line.
- **Honesty.** Pictures of the interfaces come from real runs against `ref-server --mock`. Three things are drawn: the desktop notification in `notify` (the operating system draws it, and a headless run cannot capture it), the browser tab with the alert icon (a headless page has no tab strip; icon and title are the ones `web/src/notify/attention.ts` sets), and the rejected edit in `edits` (the mock has no stale-file case; the card is marked "Illustration" and follows `docs/tools.md`).
- **Mock path.** `/tmp/ref-mock-<hex>/workspace` is random per run and shows in the terminal's splash screen and status line, the diff header and the peers list. It stays as captured. The films look away from it: the terminal replay starts after the splash and hides the last row, and the camera zooms stay clear of the diff header and the peers list. The terminal stills used in the videos are the `-clean` copies without the last row.

## How a film is built

Each scene is a list of **shots**. A shot is a stretch of video with parts on it: a screenshot window with a camera that pans and zooms (`appFrame`), a replayed terminal (`terminalView`), callouts (`highlight`), a pointer (`cursor`), typed text, captions, title cards. Shots meet through a fade or a rise. Shared shots (`intro`, `outro`) are reused by every film. The trailer reuses the feature films' key shots, cut shorter.

Source of pictures (`pnpm capture`, see [README](README.md)):

| Kind | Tool | Output |
|---|---|---|
| TUI | `ref` in a pseudo terminal against the mock backend; the output is an asciicast, replayed by xterm.js | `assets/casts/*.cast`, stills in `assets/screens/` |
| WebUI | `ref-web --mock` in headless Chromium, 1600×1000 at 2× | `assets/screens/web-*.png` |
| GUI | `ref-gui --mock` with `QT_QPA_PLATFORM=offscreen`, window grabbed from the scene graph | `assets/screens/gui-*.png` |

The ones marked README are also copied, smaller, to `docs/images/`.

## What the films show

Every film is a title card (2.5 s), the beats below, and the shared closing card (about 2.4 s). Captions are one line, in English or Chinese (one language per render). GUI pictures are used wherever the GUI shows the thing; the WebUI is used for the browser parts and for images, which only the web page can paste.

### Trailer (about 60 s)

1. The hook (12.5 s, no title card): the task typed into one session ("One agent."), the split view with helper sessions appearing and streaming, a recording ("directs a whole crew." / "They work at once."), the reports in the main session ("and report back.").
2. Logo and tagline (3.5 s).
3. The three windows side by side (5.5 s, "One backend, every window.").
4. One beat per feature, 4.5 s each: permission card, edit input with line and tag marked, `/` `@` `!` popups, context compaction, `opencode.json` typing itself, notification card. The sessions and peers films are not in the trailer; the hook shows both ideas working.
5. The finale (9.5 s): the full grid of six sessions, a "Finished" notification, "Close the window. Your crew keeps working."
6. Closing card with the repository address.

### `orchestra`: One session, a whole crew

The `orchestrate` script of the mock backend (`docs/development.md#the-mock-backend`): the main session starts four helpers (scout, fixer, tester, docs), then a reviewer; the fixer's edit waits for approval; the reports come back and the main session sums up.

1. The task typed into the main session: "Give one session the task."
2. The plan and the four `create_session` calls: "It plans, then starts helpers."
3. The split view as a recording (12.5 s): tiles appear and stream; focus moves to the fixer's tile, which asks for approval; Allow is clicked: "They all work at once." / "You only step in where it matters."
4. Terminal, app and browser side by side, all showing this crew: "Watch it from any window. Same crew."
5. The reports in the main session, then the `/peers` inbox listing them: "Reports come back to the main session."
6. The summary with a "Finished" notification: "One answer. You stayed in one place."

### `backend`: One backend, every window

1. Terminal replay of the `tools` script from the cast (half speed, after the splash): "Start a task in the terminal."
2. The terminal closes; the `ref-server` pill keeps counting: "Close the window. The session keeps working."
3. The same session in the WebUI: "Open it in a browser. Same session."
4. Terminal, native app and browser side by side: "Terminal, native app, browser. One backend."

### `sessions`: Many sessions, one screen

1. Split view of four sessions; the camera starts on the sidebar: "Every session in the sidebar, live." / "Or tile them and watch all at once."
2. The focused tile outlined: "The focused tile has its own prompt box."
3. The permission card; the sidebar row with the lock and "Needs permission" outlined, then the card's buttons: "A session that needs you stands out."

### `peers`: Sessions that talk to each other

1. One long shot of the `peer` script: the `create_session` call outlined ("The agent can start a helper session."), then the message bubble from `helper` ("Its report arrives as a message from that session.").
2. The `/peers` dialog on the Inbox tab: "Inbox and outbox show every message." (The Peers tab is not used: its list shows the mock path.)

### `permissions`: You decide what runs

1. `permission` block of `opencode.json` typing itself: "Allow, ask or deny, per tool and pattern."
2. The permission card of an `edit` call; the pointer goes to Allow: "The call waits for your answer." / "Allow once, for the session, or deny."
3. A `delete` with the delay ring and the Cancel link; the pointer clicks it and the call shows as cancelled: "A countdown gives you time to stop it."
4. A denied `curl`: "Denied calls do not stop the work."

### `edits`: Edits that cannot go stale

1. The `edit` input in the permission card, `start` and `tag` outlined: "An edit names the file tag and the lines it changes."
2. The rendered diff (red and green lines outlined): "The result is a diff you can read."
3. Illustration card after `docs/tools.md`: a read with `[src/main.rs#3F2A]`, an edit that names the tag, the file changes, and the edit is mapped through a diff or rejected: "Every read comes with a tag." / "A changed file is mapped or rejected, never overwritten blindly."

### `composer`: A prompt box with shortcuts

`/` popup with the filter `co`; `@pa` file popup; `!ls` turning the box amber, then the output in the conversation; an image pasted in the WebUI, then the answer; a queued prompt with its Remove link; the Undo link, then the draft back in the box. The GUI shots are `composer-slash`, `-at`, `-shell`, `-shell-output`, `-queue`, `before-undo` and `composer-undo`.

### `long-sessions`: Sessions that do not run out

Context meter at 190.0k / 200.0k outlined, then the "Context compacted" divider and summary; the retry status line "Retrying in 4s (attempt 2)" and the answer after attempt 3; a long answer streaming without the view jumping.

### `setup`: Your config, any model

`opencode.json` typing itself (model, agent variant, MCP server); the model dialog; the agent dialog. The MCP dialog of the mock backend says "No MCP servers configured", so MCP is shown in the config only.

### `notify`: Walk away, get called back

The permission card; a drawn notification "Waiting for permission to use edit" slides in (texts from `gui/src/notify/notification_text.cpp`); the WebUI with the drawn tab, which gets the dot and the alert icon; the notification switches in Settings.

## Not done

- The GUI's drag and drop and paste of images (the shots come from the WebUI).
- Real desktop notifications and a real browser tab strip (drawn).
- Split-view zoom as motion: the capture has a still of the focused tile, not the expand animation.

## Open questions for the project owner

- Music: none for now. If wanted, which track and licence?
