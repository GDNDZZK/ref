# qutie design

> **Historical (qutie).** ref-gui no longer talks to LLM providers itself: the `llm` and `infra` layers described here were replaced by `proto`, `client` and `session`, and the program is a client of ref-server. See ARCHITECTURE.md for the current design; the visual and motion principles below still apply.

Cross-platform GUI coding agent. C++23, Qt Quick (QML), CMake + Ninja, GPL-3.0-or-later.

## Decisions

- Dependencies come from system packages (no vcpkg). Qt floor 6.10 (required by KSyntaxHighlighting).
- Libraries: QCoro, KSyntaxHighlighting (QML `SyntaxHighlighter`), QtKeychain, dtl. Markdown via Qt's md4c, grep via external `rg`.
  A library is only `find_package`d once code uses it.
- Config: `~/.config/qutie/config.jsonc`, read-only. JSONC is lowered to JSON in place (comments and trailing commas
  become spaces, offsets preserved) and parsed by `QJsonDocument`. M1 shape: `{"endpoint": {"baseUrl", "model",
  "apiKey"?}}`. The API key is plain text in the file for now (user's call); QtKeychain comes with a settings UI.
  `max_tokens` is not sent yet; it will become a config field.
- First provider: OpenAI-compatible Chat Completions (streaming, tool calls, `reasoning_content` / `reasoning`).
  Reasoning is sent back in the field it arrived in, since replies already span several user turns.
- History matches the screen: a reply that failed or was stopped stays in the history as far as it got.
  Failures show as an `error` block (rare, must be noticed): its badge pops in with a spring and ring, and red
  tint spreads out of the badge as a hard-edged circle at constant speed (the card enters neutral). No shake: it was the only sideways, "gestural" motion in the app.
  An error arriving while the user reads further up puts a "!" badge on the jump-to-latest button.
- Streamed text is paced (`StreamPacer`): bursts are released at a steady rate, so each runs out as the next
  is due. Shown text trails the stream by at most 200 ms; stopping shows the rest at once.
- Front end first: build the UI against a mock driver, then plug in the API back end. `--mock` keeps the
  mock for UI work; it can go once the back end is settled.
- Cancellation: `std::stop_token`. Stopping completes the request before `request_stop()` returns, so an
  owner can stop from its destructor. Each ChatSession owns its driver, client and network manager (no
  singletons), so several sessions (tabs) can coexist later.

## Layers

```
app (QML + ChatSession)  ->  ui (ConversationModel, FenceSplitter, Driver: MockDriver | AgentDriver)
                             AgentDriver -> llm (config, OpenAiClient, openai_wire) -> infra (http_stream, sse, jsonc)
                             M2 puts an agent loop between AgentDriver and llm.
```

## UI

- Model rows are blocks, not messages: user, reasoning, markdown, code, tool. Code fences are split out by
  `FenceSplitter` (CommonMark rules) so each code block gets its own highlighted TextEdit.
  Rows are only appended or updated (no resets), so view transitions work.
- Visuals: light and dark, follows the system until the user toggles. Everything left-aligned; only user
  messages get a rounded bubble.
- New blocks fade in and drop `Motion.enterDrop` px into place (from above). User tuned it to 48.
- Window frame: native, untouched.
- Motion (all timing in `Motion.qml`): exponential ease-out (`OutExpo`) almost everywhere, no overshoot in
  frequent interactions. Overshoot only for rare moments (theme toggle, `OutBack`) or for self-contained
  flourishes that move nothing else (tool completion: elastic spring plus a ring).
- Theme switch: snapshot crossfade (grab the page, swap colors, fade the snapshot out).
- Turns: each user message after the first opens with a full-width divider; the bubble is accent-tinted.
- Auto-follow: only user gestures (movementStarted/Ended, scroll bar drag) change `following`; never compare
  contentY against contentHeight, which ListView only estimates. Following chases the end with a per-frame
  exponential ease (`Motion.followLag`), not jumps.
- Continuous targets (streaming block height, scroll follow) use a per-frame exponential chase via
  FrameAnimation. `Behavior` only for discrete targets: it restarts on every target change and stalls
  when the target itself animates.
- Foldable blocks (reasoning, tool) keep their header pinned to the view's top while expanded, until the
  block's end pushes it out. Folding from a pinned header scrolls back with the fold, so the header stays put.
- Hold: during a turn the end never moves up; room the content gives back is kept as footer space and
  released with an animation when the turn ends. Prevents bouncing when wrapped lines unwrap.

## Progress

- [x] M0 skeleton, JSONC, SSE parser (with tests)
- [x] UI v1 against MockDriver: blocks, streaming, code highlighting, tool cards, reasoning fold, theme toggle
- [x] UI v1 iterated by hand: turn dividers, entry drop, smooth follow, hold, per-frame height chase,
      slow/long mock scripts. Phase closed; user reviewing code.
- [x] Review round 1: drawn chevron icons, themed scroll bar, exact geometry (all rows kept), wheel
      chase (Wayland reports wheels as touchpad), entrance as transform, whole-pixel scroll chase.
- [ ] M1 http_stream + OpenAiClient + AgentDriver + error block (builds clean with GCC 16 and Clang 23 on
      Qt 6.12; tests pass, also under ASan+UBSan). First real-server run done by the user; fixes after it:
      StreamPacer, new error animation, error badge on jump-to-latest (awaiting the user's visual check)

See `HANDOFF.md` (agent context) and `ARCHITECTURE.md` (human-oriented walkthrough).

---

# ref-gui conversation area (zone A): visual spec

The GUI is the visual authority; the WebUI follows. Sizes are logical pixels. Names in `code` are constants:
colours go to `Theme.qml`, timings to `Motion.qml`, the rest are literal properties at the top of the component
that uses them. Icons are `Icon { name; size }` (assets/icons, 16 next to text, 20 in buttons), never scaled.
Existing metrics stay: reading column `Theme.contentWidth` 760, `blockSpacing` 14, `turnGap` 18, card radius
`Theme.radius` 10, header height 38, card padding 14.

## New theme colours (light / dark)

| Name | Light | Dark | Use |
|---|---|---|---|
| `warning` | #b86e00 | #e5b567 | permission, delay, retry, interrupted, shell/reminder bar |
| `info` | #2a7fb5 | #6fb7e0 | messages from other sessions, hunk headers, tool input |
| `diffAdded` / `diffAddedBackground` | #1f7a4d / #e3f3e9 | #6cc28a / #1d2f27 | added lines |
| `diffRemoved` / `diffRemovedBackground` | #b3303f / #fbe6e8 | #ec6f73 / #33202a | removed lines |
| `matchBackground` | #fff1c2 | #3a3420 | grep match lines |
| `agentPalette` (5) | #5c9cf5 #9d7cd8 #2e9d5b #d9822b #d23f45 | #7aa2f7 #b49bf0 #6cc28a #f0a86a #ec6f73 | agent colour when the agent has none; index = position in the agent list, else hash of the name |

`Theme.staleOpacity` 0.55: messages before the last compaction.

## Tool card

Header row (38): `[state 16] [type icon 16] name purpose detail ... status duration time [chevron]`, 10 between items.
- `name` mono 13 bold; `purpose` 13 text; `detail` (one line from the input: command, path, pattern, url, `to`, `n/m done`) mono 12
  `textMuted`, elided; status/duration/time 12 `textMuted` (status in the state's colour).
- Time is the start clock `HH:mm:ss`, shown at widths >= 560, otherwise dropped.
- Type icon (16, `textMuted`): read file-text, edit pencil, write file-plus, delete trash, bash terminal, grep search,
  find folder-search, webfetch globe, todo todo, create_session chat-plus, send_message / message_status chat-send,
  list_sessions chat-list, close_session chat-x, read_session chat, MCP tools mcp, anything else tool.
- The card is hoverable when it has details (`surfaceHover`); a click toggles it. Expanded state is stored per
  part in the model (`expanded` role), default `detailsExpanded`; `awaiting_permission` forces it open.

### The eight states

| State (`toolState`) | State slot (16 x 16) | Right-hand text | Card |
|---|---|---|---|
| `streaming` | 8 dot `accent`, pulsing (`Motion.pulse`, dimmed 0.5) | "writing…" | normal; detail line follows the streamed input |
| `awaiting_permission` | `lock` icon, `warning` | "Needs permission" (`warning`) | border `warning` at 60%; permission section open |
| `delayed` | countdown ring (below), `warning` | "Runs in 4s" (`warning`) and a **Cancel** text button | border `warning` at 60% |
| `running` | 8 dot `accent`, pulsing | "running 12s" (`accent`, updates each second) | live output tail when expanded |
| `done` | 8 dot `success` | duration, e.g. "1.2 s" | the dot plays Flourish when it arrives from running/streaming |
| `error` | 8 dot `danger` | "error 1.2 s" (`danger`) | collapsed: first output line under the header (`└ `, `danger`, one line, elided) |
| `denied` | ring 8, 1.5 stroke `textMuted`, no fill | "denied" | name struck through, text `textMuted` |
| `cancelled` | ring 8, 1.5 stroke `textMuted`, no fill | "cancelled" | name and purpose `textMuted` |

Colour changes between states cross-fade over `Motion.standard`; cards created in a final state show no Flourish.

**Countdown ring** (delayed): 16 circle, stroke 2. Track `border`; arc `warning` with round caps, starting at 12 o'clock,
shrinking anticlockwise to nothing: sweep = remaining / total * 360, linear. `total` = the remaining time when the
card first saw the state (a card that appears mid-delay starts partly drawn). Redrawn every frame while `delayed`;
the number in the text uses `ceil(remaining / 1000)` and ticks each second. Reaching zero swaps to `running`.

## Permission section

Inside the card, under the details, separated by a 1 divider. Height animates over `Motion.standard`.
- Prominent fields first, as the TUI does: `command` as `$ cmd` mono 13 bold on `codeBackground` (radius 6, padding 8 x 10, wrapped),
  `path` as a mono 13 `info` line. The full input follows in the card's body (forced open).
- Button row, padding 10 x 14, spacing 8, height 30 each (radius `radiusSmall`): icon 16, label 13, key cap on the right.
  **Allow** (`check`, `success`, key `a`), **Allow for session** (`check-all`, `accent`, key `s`), **Deny** (`close`, `danger`, key `d`).
  Style: 1 border in the colour at 50%, text in the colour, fill = colour at 12% on hover and 22% pressed.
  Key cap: 18 x 18, radius 4, 1 border in the same colour, mono 11 letter.
- Right end: "+N more waiting" 12 `textMuted` when other requests of this session are pending.
- Keys act on the oldest pending request of the session: `a` or Enter allow, `s` always, `d` or Esc deny. Keys are ignored for
  `permissionGuard` = 350 ms after the request appears (the user was still typing); letters then no longer reach the composer
  until all requests are answered.
- Answered: the section collapses over `Motion.standard`; the state moves on with the next event.
- A request outside the view puts a `warning` AlertBadge ("?") on the jump-to-latest button, like errors do.

## Tool content (expanded body)

Body padding 10 x 14. A section has a label row (11, `textMuted`, medium weight, 6 below) and sections are
separated by 12. Text is mono 12, line height 18, selectable. Output over `previewLines` = 200 lines is cut with a
text button "Show N more lines" (`accent`, 12) that shows all. Copy: a `copy` icon button (16, top right of a code or
output area, visible on hover, turns to `check` for `Motion.slow` after copying).

| Tool | Body |
|---|---|
| generic / unknown | "Input" (pretty JSON, `info`) then "Output" (`textMuted`; `danger` when the tool failed; "(no output)" when empty) |
| `edit`, `write` | Path row (`meta.path`, mono 12 text) with `+added` (`diffAdded`) and `-removed` (`diffRemoved`) counts, then the diff. Without `meta.diff`: generic |
| `bash` | Command box (`$ cmd`, as in the permission section, with copy); then "Output" with an exit chip: `exit 0` `textMuted`, otherwise `danger`, plus "timed out" / "cancelled" / "truncated" chips from `meta`; while running the live output (`meta.live_output`) as a tail of 20 lines with "N earlier lines" above |
| `read` | Path row with the tag dim; lines as gutter + text (below) |
| `grep` | One group per file: path row (mono 12 bold) and gutter + text lines; match lines (`*`) on `matchBackground`, context lines `textMuted`, `...` rows as a dim "⋯" separator. Footer chip "N matches in M files" (+ "truncated") from `meta` |
| `find` | One row per path (height 24): `file` or `folder` icon 16 `textMuted`, directory part `textMuted`, name `text`; footer chip "N results" (+ "truncated") |
| `webfetch` | URL row (mono 12 `info`) with `external-link` icon button, status and size chips from `meta`, then the output |
| `todo` | Not foldable: the list is always shown under the header (see below) |
| `send_message`, `create_session` | Recipient row: `chat-send` / `chat-plus` icon 16 `info` + name (`info`, bold 13); the message text (`input.message` / `input.prompt`) in prose 13 on a 3 `info` left bar (padding-left 10); then "Output" |
| `read_session`, `list_sessions`, `message_status`, `close_session` | Session name row (`chat` / `chat-list` / `chat-send` / `chat-x` icon + name), then "Output" |
| Images (`images` of any tool, e.g. `read` of a picture) | Thumbnail row below the sections (below) |

**Gutter + text** (read, grep): line numbers right aligned in a column as wide as the longest number (mono 11, `textMuted` 70%),
12 gap, then the text. Not wrapped; the area scrolls horizontally with a thin bar when a line is wider.

**Diff**: unified diff from `meta.diff`, mono 12, line height 18, no wrapping (horizontal scroll). Row backgrounds fill the full width:
`+` lines `diffAddedBackground` / text `diffAdded`; `-` lines `diffRemovedBackground` / `diffRemoved`; `@@` rows `info` on `surface`;
`---` / `+++` rows `textMuted` bold; context `textMuted`. A 16 wide sign column keeps `+` / `-` aligned.

**Todo**: rows of height 26, left padding 14: marker 16, 8 gap, text 13. `pending` ring 1.5 `textMuted`; `in_progress` ring `accent`
with a pulsing 8 dot (`Motion.pulse`); `completed` `success` disc with a white tick (`check` 16 as ink on the disc), text `textMuted`
struck through; `cancelled` `close` icon `textMuted`, text `textMuted` struck through. The header detail reads "2/5 done".
Rows animate their marker colour over `Motion.standard`.

**Image thumbnails**: row under the sections, spacing 8, each at most 160 x 96 (aspect kept, radius 6, 1 `border`), loaded with GetImage.
A placeholder (`surface`, `image` icon 20 centred) shows until the bytes arrive; a failed load shows `error` icon `danger`. Hover
dims 12% and shows `maximize` icon 20 white in the corner. A click opens the **lightbox**: a scrim `Qt.rgba(0, 0, 0, 0.72)` over the window,
the picture fitted to the window minus `lightboxMargin` = 48, name and size caption below (12, white 80%), a 34 round close button
(`close` 20) top right; Esc, a click on the scrim or on the button close it. Open/close: fade and scale 0.96 to 1 over `Motion.standard`.
Pictures attached to a user message use the same thumbnails (72 high) under the bubble.

## Message sources and meta row

User-role messages by `source`:
- `human`: unchanged (accent-tinted bubble, `userBubble`).
- `peer`: `surface` bubble with a 3 `info` bar on the left; header row `chat-send` icon 16 + `message from "name"` (12 bold `info`).
- `spawn`: as `peer`; header `chat-plus` + `created by "name"`.
- `reminder`: `surface` bubble, bar `warning`; header `retry` icon + `reminder` (12 bold `warning`).
- `shell`: `codeBackground` bubble, bar `warning`; header `terminal` icon + `shell` (12 bold `warning`); body is the Markdown of the output (mono).
- `compaction`: the divider (below).
Bubbles keep the human bubble's radius and padding; the header sits 6 above the text.

**Meta row** under every message (height 24, 12, `textMuted`, gap 8 with "·" separators), full opacity on hover of the message, 60% otherwise:
- User: `HH:mm:ss` · **Undo** (`undo` 16 + "Undo", human messages only) · **Copy** (`copy` 16 + "Copy"). Right aligned in the bubble column.
- Assistant: agent dot (8, agent colour; pulsing and elapsed time while streaming) and agent name (titlecase, agent colour; `textMuted` when it ended with an error)
  · model name and variant (variant `warning`) · `HH:mm:ss -> HH:mm:ss (1.2 s)` · `up 12.3k down 456` (up = input + cache read + cache write) · `38 tok/s` (`textMuted` 60%, shown when
  output > 0 and the span > 0) · `interrupted` (`warning`, when the error is "aborted"). **Copy** is at the right end. The tooltip of the time shows the full date.
- Copy writes the raw text of the message (text parts joined by a blank line); the icon becomes `check` for `Motion.slow`.
- Undo asks nothing and removes this message and all after it (`Undo` with `message_id`); the text and pictures go back to the input box.

## Compaction

The message with source `compaction` is a full-width divider (1 `border` line on both sides) with a centred pill:
`history` icon 16 `accent`, "Context compacted" (13 bold `accent`), the time (12 `textMuted`) and a chevron. A click unfolds the summary under it
(Markdown 13 `textMuted` on a 2 `accent` left bar, padding-left 12, unfolding over `Motion.standard`). Copy sits at the line's right end
(visible on hover). Under the divider of the **last** compaction, centred, 12 italic `textMuted` 70%: "Messages above stay visible but are no longer sent to the model".
Every message before the last compaction is drawn at `Theme.staleOpacity`.

## Removed, Reset, older messages

- **Removed** (undo, retry): the block fades to 0 over `Motion.quick`, then its height closes over `Motion.standard` with `Motion.ease` (rows below move up with it).
  A message's meta row goes with it.
- **Reset** (compaction, undo): only the rows that differ change. Rows that stay do not move; removed rows leave as above, new rows enter as new
  blocks do (fade and drop `Motion.enterDrop`). A snapshot (attaching, switching session) shows no animation.
- **Older messages**: when `moreBefore` is true, a pill "Load older messages" (12 `textMuted`, `history` icon 16, height 28, centred) is the first row;
  scrolling within `olderTriggerDistance` = 400 of the top loads the next page by itself. While loading the pill shows a pulsing dot instead of the icon.
  Prepended rows do not animate and the view keeps what the user reads where it is (the scroll position moves by the height added above).
  A failed load puts the error text in the pill (`danger`) and a click retries.

## Status line (above the input box)

A row of height 28 at the bottom of the chat, in the reading column, 12 `textMuted`, always present so nothing jumps.
- Left, by `activity`: idle nothing; `thinking` pulsing 8 dot `accent` + "Working" with a shimmer; `running` dot + "Running tools";
  `waiting_permission` `lock` icon + "Waiting for your permission" (`warning`); `compacting` dot + "Compacting context…" (shimmer `accent`);
  `retrying` `retry` icon + "Retrying in 4s (attempt 2): <error, one line, elided>" (`warning`; the number counts down each second).
  Then " · 1m 12s" for the run (from `busySince`, each second; hidden under 1 s).
- Right: context use "12.4k / 200k" with a bar 48 x 4 (radius 2, track `border`, fill `accent`); the fill turns `warning` from `contextRecommend`
  and `danger` from `contextAuto`; the text is the same colour then. Hidden while `contextMax` is 0. Tooltip: the four numbers.
- A shimmer is a highlight sweeping over the label: `shimmerPeriod` = 1600 ms for one sweep, highlight width 3 characters, colours `textMuted` to `text`.

## Errors

- An assistant message error (not "aborted") is an `error` block under the message (existing ErrorBlock).
- A session `Error` event is a block at the end of the list (same ErrorBlock, no message), unless its text equals the error of the last message.
  It leaves with the next prompt the user sends. Details in the TUI: view.error, tail rows.
- Spread speed of the error tint `spreadSpeed` = 1.5 px/ms (constant, linear, starts `Motion.quick` after the card lands).

## Constants added (for the WebUI's list)

Theme: `warning`, `info`, `diffAdded`, `diffAddedBackground`, `diffRemoved`, `diffRemovedBackground`, `matchBackground`, `agentPalette`, `staleOpacity`.
Component literals: `permissionGuard` 350 ms, `previewLines` 200, `lightboxMargin` 48, `olderTriggerDistance` 400, `shimmerPeriod` 1600,
`thumbnailMaxWidth` 160, `thumbnailMaxHeight` 96, `userThumbnailHeight` 72, `liveTailLines` 20, `spreadSpeed` 1.5.
