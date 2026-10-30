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

## Composer and notifications (C area)

Visual spec for the input box, its pop-up lists, attachments, the queue strip and the notification
settings. Sizes are logical px. Motion names refer to `Motion.qml`; "OutExpo" is `Motion.ease`. Values marked
(new) are new literals: they sit at the top of the component that uses them (the WebUI generator reads them
from there, so keep them literals) or in `Theme.qml` / `Motion.qml` where noted. Nothing here overshoots:
every interaction is frequent. Icons come from `assets/icons` through the icon provider
(`image://icons/<name>-<size>?color=<hex>`), at their grid size.

### Box layout

```
 queue strip (only when something is queued)                      queueGap 8
 +------------------------------------------------------------+
 | attachment strip (only with attachments)                   |  padding 8
 | text area  (1 line .. maxInputHeight 220, grows with text)  |
 | [agent] [model] [variant]   hint                       (->) |  tagRow 32
 +------------------------------------------------------------+
```

- The box keeps `Theme.surface`, `Theme.radiusLarge`, border `Theme.border` (`Theme.accent` while the text
  area has focus), and `padding` 8. Its width is the reading column. The text area spans the full inner width
  (the send button no longer takes a column from it).
- Height = padding + strip (0, or `thumbSize` + `thumbGap`) + text height + 4 + `tagRowHeight` (new, 32) +
  padding. It animates with `Motion.standard` / OutExpo, as before.
- The send button (32 x 32, unchanged, all its motion unchanged) stays in the bottom-right corner of the box,
  in the tag row. `sendButtonCenterX` still gives the center line the jump-to-latest button aligns with. That
  button anchors to the top of the whole Composer item, so it rises above the queue strip when there is one.
- Placeholder: "Message ref" / "Waiting for the ref backend…" as before; "Type to queue a message…" while the
  session is busy; "Run a shell command…" in shell mode.

### Tags (agent, model, variant)

- A pill per tag, left-aligned in the tag row, `chipGap` (new, 4) apart. Height `chipHeight` (new, 28), full
  radius, horizontal padding `chipPaddingX` (new, 10). Content: a 16 px icon (`agent`, `model`, `variant`),
  `chipIconGap` (new, 6), then the label in `Theme.smallFontSize`, `Theme.textMuted`.
- The agent icon is tinted with the agent's color; the label turns `Theme.text` on hover. Model and variant icons
  stay `Theme.textMuted`. Hover: pill background `Theme.surfaceHover`, `Motion.quick` / OutExpo.
- Agent color: the configured `color` of the agent (`#rrggbb`, `#rgb`, or one of the names below), otherwise the
  palette entry at the agent's index in the agent list (modulo 5). `Theme.agentColor(spec, index)` resolves it.
  Names: primary/orange, secondary/blue, accent/purple, error/red, warning/yellow, success/green, info/cyan.
  Palette order: blue, purple, green, orange, red. Same names as the TUI.
- Labels: agent name; model display name (the provider is in the tooltip); variant name, or "default" when the
  model has variants and none is chosen. The variant pill is absent when the model has no variants.
- Clicking the agent or model pill opens the agent / model dialog (signals `agentClicked`, `modelClicked`);
  clicking the variant pill cycles it. Tooltips: "Agent (Tab)", "Model", "Variant (Ctrl+T)".
- Morph when a value changes (Tab, Shift+Tab, Ctrl+T, a dialog, `/model`, another client): the pill width
  follows its content, `Motion.standard` / OutExpo. The old label fades out while moving up `labelSlide`
  (new, 6) and the new one fades in from `labelSlide` below, both `Motion.standard` / OutExpo. The icon color
  eases to the new agent color, `Motion.standard`. Pills to the right slide along with the width. Switching
  agent often changes the model too: both pills morph in the same frame.
- Variant pill appearing / disappearing (model changes): width 0 <-> content and opacity 0 <-> 1,
  `Motion.standard` / OutExpo.
- While no session is open the tag row is empty. Changing a tag shows the new value only when the backend
  confirms it (the Info event); the morph starts then.

### Shell mode (`!`)

- Entering: `!` typed as the first character of an empty box is swallowed and sets shell mode (as in the TUI).
  Backspace on an empty box or Esc leaves it. Choosing a `!cmd` entry from the history enters it too.
- Look: box background `Theme.shellSurface` (new), border `Theme.shellBorder` (new; also while focused), both
  eased with `Motion.standard`. The send button's active color becomes `Theme.shellBorder` and its arrow stays.
  A shell pill (`terminal` icon, label "Shell", `Theme.shellBorder`) grows in at the start of the tag row:
  width 0 -> content with a fade, `Motion.standard` / OutExpo; the other pills slide right.
- Colors: dark `shellSurface #2a2419`, `shellBorder #f5a742` (the TUI's warning amber); light `shellSurface
  #fbf3e3`, `shellBorder #d9922b`.
- Shell mode ignores `/` and `@` lists. Sending runs the command in the session directory and does not queue.

### Pop-up lists (`/` commands, `@` files)

- Opens above the box, `popupGap` (new, 8) over its top edge, same width as the box, over the queue strip.
  Background `Theme.surface`, border `Theme.border`, radius `Theme.radius`, inner padding `popupPadding` (new, 4).
- Rows `popupRowHeight` (new, 36), radius `Theme.radiusSmall`, at most `popupMaxRows` (new, 8) visible, then it
  scrolls. Height = rows x 36 + 2 x 4 and follows the result count with `Motion.standard` / OutExpo.
- Row, `/`: label `/name` (`Theme.text`, `Theme.fontSize`); an alias that matched shows as `/alias -> /name`;
  right-aligned description in `Theme.smallFontSize`, `Theme.textMuted`, elided; the command's shortcut, when
  it has one, after it. A `slash` icon (16, `Theme.textMuted`) leads each row.
- Row, `@`: leading `file` or `folder` icon (16); the path in `Theme.monoFamily` at `Theme.codeFontSize`, the
  directory part `Theme.textMuted`, the file name `Theme.text`. Directories end with `/`. No description.
- The selected row has `Theme.surfaceHover` behind it. One highlight rectangle moves between rows,
  `Motion.quick` / OutExpo; the pointer moves the selection on hover.
- Open: opacity 0 -> 1 and a rise of `popupGap` px (y +8 -> 0), `Motion.standard` / OutExpo. Close: fade only,
  `Motion.quick`. While results update the list stays; it closes when there are none.
- The list is for the word under the cursor: `/` only at the start of the text with no space yet; `@` after
  whitespace or at the start, up to the next whitespace. File results arrive about 80 ms after the last keystroke;
  stale answers are dropped.
- Keys while open: Up/Down and Ctrl+P/N move (wrapping), Tab accepts, Enter accepts (on a fully typed command it
  runs it, as in the TUI), Esc closes it until the text changes. Clicking a row accepts it.

### Attachments

- Strip inside the box above the text area, `thumbGap` (new, 8) between thumbnails, each `thumbSize` (new, 56)
  square, `Theme.radius` corners, image scaled to fill and clipped. The strip scrolls sideways when full.
- Each thumbnail has a remove button in its top-right corner, 4 px inset: `attachmentRemoveSize` (new, 18)
  disc in `Theme.background` at 85 % opacity with a 8 px drawn cross in `Theme.text`. It fades in on hover or
  keyboard focus (`Motion.quick`) and is always shown on touch screens. Tooltip: the file name.
- Add: scale 0.8 -> 1 and fade in, `Motion.standard` / OutExpo; the strip grows 0 -> 56 + 8 with it. Remove:
  scale 1 -> 0.8 and fade out, `Motion.quick`; the others slide over with `Motion.standard`.
- Sources: Ctrl+V with an image on the clipboard, pasting the path or `file://` URL of an image file (png, jpg,
  gif, webp; at most 20 MiB), dropping image files on the box. Restored drafts (undo, withdrawing a queued
  prompt) bring their images back as thumbnails.
- Drag over the box: border `Theme.accent` and a centered label "Drop images to attach" in `Theme.textMuted`
  over a `Theme.surface` veil at 90 % opacity, `Motion.quick`.

### Queue strip

- A prompt sent while the session is busy waits on the server and is listed above the box, `queueGap` (new, 8)
  apart: a container with `Theme.surface`, border `Theme.border`, radius `Theme.radius`, clipped, rows separated
  by a 1 px `Theme.border` line.
- Row height `queueRowHeight` (new, 32), padding 12. Left to right: `clock` icon (16, `Theme.textMuted`), the text
  on one line (`Theme.smallFontSize`, `Theme.text`, elided), `image` icon with the count when it has images,
  the sender ("from <name>", `Theme.textMuted`) for messages of other sessions and reminders, and for your own
  prompts a "Remove" button: `recall` icon (16) + label, `Theme.textMuted`, `Theme.text` on hover. Messages
  from other sessions and reminders have no button.
- At most `queueMaxRows` (new, 4) rows are visible, then the strip scrolls.
- Row enters: height 0 -> 32 and fade in, `Motion.standard` / OutExpo. Row leaves: height -> 0 and fade out,
  `Motion.standard`; the rows below slide up with it. The container height follows the row heights, so the box
  and everything above it moves with the same curve.
- After Remove is clicked the row dims to 50 % until the server answers. If it was withdrawn, the row leaves and
  its text and images go back into the box (after a blank line when the box is not empty); if it had been
  delivered meanwhile, the row stays until its message arrives and a toast says "Already sent; the prompt can no
  longer be removed".

### Interrupt hint

- Esc while the session is busy and no list is open shows "Press Esc again to interrupt" in the tag row, right of
  the pills and left of the send button: `Theme.smallFontSize`, `Theme.text`, fading in with `Motion.quick`.
  A second Esc inside `Motion.interruptWindow` (new, 1500 ms) interrupts; after the window the hint fades out
  and the next Esc arms it again. A delayed tool call is cancelled by a single Esc (as in the TUI); that rule
  belongs to the transcript area, the composer only forwards Esc.

### Keys in the box

| Key | Effect |
|---|---|
| Enter | Send (or run the command / shell line). Not while an input method is composing. |
| Shift+Enter, Ctrl+J | New line |
| Tab / Shift+Tab | Next / previous agent (accepts the list entry while a list is open) |
| Ctrl+T | Next variant of the model |
| Ctrl+V | Paste; an image on the clipboard becomes an attachment |
| Up / Down | Prompt history: Up when the box is empty, the cursor is at the very start, or history is being browsed; Down while browsing and on the last line. Otherwise they move the cursor. |
| Esc Esc | Interrupt the session |
| Esc | Close the list; leave shell mode |
| Ctrl+C | With a selection copy; otherwise clear the box (text of 20+ characters goes to the history first) |

### Notification settings

`NotificationSettingsSection.qml` is a block for the settings panel. Rows are 40 px high (`Theme.fontSize` label
left, switch right), `bell` icon (20, `Theme.textMuted`) in the section header "Notifications":

1. "Show system notifications" (master switch);
2. "A tool needs permission", "A turn finished", "A turn ended with an error", indented 24 px, 40 % opacity
   and not clickable while the master switch is off (`Motion.standard`). Other sessions' messages never notify.

When the system cannot show notifications the block adds a note under the master switch in
`Theme.smallFontSize` / `Theme.textMuted`: "Not available on this system. The taskbar flashes instead." (the
reason from the platform layer is the tooltip). The switches still work: they choose when the flashing happens.

### New visual constants

- `Theme.qml`: `shellSurface`, `shellBorder`, `agentOrange`, `agentBlue`, `agentPurple`, `agentRed`,
  `agentYellow`, `agentGreen`, `agentCyan`, function `agentColor(spec, index)`.
- `Motion.qml`: `interruptWindow` (1500).
- Composer.qml literals: `tagRowHeight` 32, `chipHeight` 28, `chipPaddingX` 10, `chipIconGap` 6, `chipGap` 4,
  `labelSlide` 6, `thumbSize` 56, `thumbGap` 8, `attachmentRemoveSize` 18, `popupGap` 8, `popupPadding` 4,
  `popupRowHeight` 36, `popupMaxRows` 8, `queueGap` 8, `queueRowHeight` 32, `queueMaxRows` 4.
