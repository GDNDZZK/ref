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
  released with an animation when the turn ends. Prevents bouncing when wrapped lines unwrap. The hold rests for
  `Motion.slow` whenever the number of pending permission requests changes: a card that closes after its answer can be
  taller than the view, and holding its old height would leave the view blank.

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

# ref-gui shell (area B): visual spec

Everything here follows the qutie rules above: `Motion.quick/standard/slow` with `OutExpo`, no overshoot in
frequent interactions, colors only from `Theme`. New constants are listed per section and collected in the last
table. "Face" is the pressed-scale item (handlers stay on an unscaled root, see CircleButton).

## Window layout

```
┌───────────────┬──────────────────────────────────────────────┐
│ Sidebar 264   │  Content: SessionPane  |  SplitView          │
│ (collapsible) │            [split] [theme]  (top right, 14px) │
└───────────────┴──────────────────────────────────────────────┘
```

- The sidebar is inline. The content area is whatever is left; the reading column stays centred in it
  (`ChatView.columnWidth`), so collapsing re-centres the text. Window `minimumWidth` 640, `minimumHeight` 420.
- Top right, as in qutie: ThemeToggle at 14px from the edges. A second 34px CircleButton for split view sits 8px to
  its left (`split-20`). Both stay under the dialog scrim.
- Top left, only while the sidebar is collapsed: two 34px CircleButtons at 14px, 8px apart: `sidebar-20` (expand) and
  `plus-20` (new session). They fade in with `Motion.standard` as the sidebar closes.
- Switching the shown session (sidebar click, palette, shortcuts, notification click) is a snapshot crossfade of the
  content area, the same mechanism as the theme switch: grab, swap `session`, fade the snapshot out with
  `Motion.standard` + `Motion.ease`. No slide.
- Layers above the content, bottom to top: tiles' zoom overlay, banner, toasts, scrim, dialog / palette.

## Sidebar

Collapse: the sidebar item's width animates 264 → 0 (`Motion.standard`, `Motion.ease`). Its content has a fixed
width of 264, is clipped by the sidebar and is anchored to the right edge, so it slides out to the left. The window
size does not change. State is kept in `Settings.sidebarVisible`.

Surface: `Theme.surface`, a 1px `Theme.border` line on the right edge.

```
header   52   "ref" (Theme.fontSize+1, bold)                      [sidebar-20 button 32]
         44   [ + New session ........................ ]  [ ▾ ]    new = 36px high pill, ▾ = folder-open-20 button
running  28   section label "Running"  (smallFontSize, textMuted)
         56×n session rows (the list is as high as its rows; it scrolls only when the history would get less than 3 rows: max(free height − 3 × 44, 3 rows))
history  28   section label "History"
         32   search field
         44×n history rows
footer   44   [mcp-16] MCP ●●●● ........................ [sliders-20 button 32]
```

Padding: rows are inset 8px from the sidebar edges (`sidebarPadding`), 2px apart. Section labels start at x = 18.

**New session button**: surface fill, `Theme.border` border, radius 18, text `Theme.text` at `smallFontSize+1`
with `plus-16` before it. Hover `surfaceHover`, press scale 0.96 (face only), `Motion.quick`. The ▾ button next to it
(`folder-open-20`, 36×36 same style) starts "New session in folder…" (D10).

**Running session row** (56px high, radius `Theme.radius`):

```
 ● title ........................... 2/5      line 1: state dot, title (fontSize-2 = 13, text), right: todo progress or marker
   dirname · 1m 23s                           line 2: smallFontSize, textMuted
```

- Left padding 12, dot 8px at x = 12, title at x = 30. Right padding 12.
- Title: session `title`, falling back to `name`. Elided right.
- Line 2: last path component of `cwd`; while the session works ` · ` and the turn's running time (`busySince`, ticks
  every second, format `45s`, `3m 05s`, `1h 02m`).
- Todo progress: `done/total` in `smallFontSize` textMuted (cancelled items are not counted, like the TUI); when all
  are done `✓done/total` drawn as `check-16` + text in `Theme.success`. Hidden without todos.
- Hover: `surfaceHover` fill, and a 22px `close-16` button appears at the right edge, replacing the progress
  text (cross-fade `Motion.quick`). Press scales the face to 0.98.
- Current session: `userBubble` fill, `userBubbleBorder` border (the accent tint of the user bubbles). Fill and border
  animate with `Motion.quick`.
- Right click: context menu Rename / Close. Double click on the title: inline rename (a TextField in place of the
  title, Enter commits, Esc cancels, losing focus commits).

State dot (8px, `SessionDot.qml`):

| State | Dot | Row |
|---|---|---|
| idle | `textMuted` at opacity 0.5 | plain |
| busy / running | `accent`, `Pulse` (`Motion.pulse`, dimmed 0.5) | plain |
| compacting | `accent`, `Pulse` | line 2: "Compacting" |
| retrying | `warning`, `Pulse` | line 2: "Retrying" in `warning` |
| waiting permission | `warning`, `Pulse` | **attention**: `warningSurface` fill, `warningBorder` border, `lock-16` in `warning` instead of the progress, line 2 "Needs permission" in `warning` |
| last turn ended with an error | `danger`, solid | **attention**: `AlertBadge` 14px instead of the progress, line 2 the error text elided in `danger` |

- Turn finished (busy → idle): the dot plays `Flourish.play()` with `Theme.success` and then settles to the idle
  color (`Motion.standard`). Waiting → anything else: the row's attention fill fades out (`Motion.standard`).
- Entering the attention state: the lock / `AlertBadge` appears with `Flourish.enter()` (colors `warning` /
  `danger`), the fill fades in with `Motion.standard`. A second error while it shows plays `Flourish.play()`.
- The error mark clears when the backend clears `last_error` (next turn starts).
- List transitions (`ListView`; the rows have a fixed height, so the `add` transition problem from the transcript
  does not apply): `add`: opacity 0 → 1 and scale 0.96 → 1, `Motion.standard`; `remove`: opacity → 0, scale → 0.96,
  `Motion.standard` with `Motion.exitEase`; `displaced`: y with `Motion.standard` + `Motion.ease`.

**History**: the search field is 32px high, radius 16, `surface` fill, `border` border (`accent` while focused,
`Motion.standard`), `search-16` on the left, `close-16` clear button on the right when not empty, placeholder
"Search history". Filtering is client side, case-insensitive, on title, name, directory and id; the list changes
instantly (no per-row animation while typing). Live sessions are not repeated here.
History row, 44px high, same inset as running rows: title (13, `Theme.text`), line 2 (`smallFontSize`,
textMuted): `dirname · relative time`. Hover `surfaceHover`. Right click: Delete (asks to confirm). Empty result:
centred muted text "No matching sessions"; still loading: "Loading…".
Relative time: under a minute "just now", `5m ago`, `3h ago`, `Yesterday`, then `Mar 4`, `Mar 4, 2025` from another year.

**MCP footer**: `mcp-16` (textMuted), the text "MCP", then one 6px dot per server, 4px apart, at most 6 then
"+n" (`smallFontSize`). Dot color: connected `success`, connecting `accent` with `Pulse`, failed `danger`, disabled
`textMuted` at 0.5. Hidden entirely when there are no servers (the settings button stays). Click opens the MCP dialog.
The settings button is `sliders-20` in a 32px square face, hover `surfaceHover`.

## Split view

Replaces the SessionPane in the content area; the sidebar stays and marks the tiles' sessions with a 2px
`accent` bar at the row's left edge (`isTile`). The selected row (`isCurrent`) is the session of the focused tile, so it
follows a click, `Alt+arrows` and a click on the row; `Workspace.currentId` (what comes back when split view ends) does not move. Tiles are the live sessions nobody is attached to plus the current
one (TUI rule). Empty: centred muted text "No background sessions" and a "New session" text button below.

Grid. Outer padding `tilePadding` 16, gap `tileGap` 12. With `n` tiles in a content area of width W and height H:

```
cols = min( ceil(sqrt(n)), max(1, floor((W - 2*pad + gap) / (tileMinWidth + gap))) )
rows = ceil(n / cols)
tile height = max(tileMinHeight, (H - 2*pad - (rows-1)*gap) / rows)    // the grid scrolls vertically if taller than H
```

Tiles fill rows in `created_at` order. The tiles of a partly filled last row share its width equally. When the
whole grid is higher than H it scrolls with the themed scroll bar; there are no pages.

Tile: radius `Theme.radius`, `Theme.background` fill, 1px `Theme.border` border (`accent` for the focused tile,
`Motion.standard`), clipped. Header 36px, `surface` fill, 1px `border` line below:

```
 ● title ................ 2/5  1m 23s  [maximize-20 button 28]     state dot, title (13), todo progress, turn time, zoom
```

The dot, progress, time and the attention treatments are the sidebar row's (permission: header in `warningSurface`
with `lock-16`; error: `AlertBadge`). The body is a `SessionPane` with `compact: true`: reading column =
tile width − 2 × 16; the composer's outer margin is 10 instead of `pagePadding`; only the focused (or zoomed) tile
shows the input box with its chip row and attachment strip (`Composer.boxShown`), as the TUI does: the others hide it
(height → 0 and opacity → 0 over `Motion.standard`, `Motion.ease`; the draft, attachments, shell mode and caret are kept,
the item is not destroyed), while the status line and the queue strip stay. The queue strip sits directly above the
box's place, so only the box's height moves. A tile that follows the end of its conversation keeps the end on the
bottom edge while its height changes (`ChatView.onHeightChanged`). Focusing a tile (click, `Alt+arrows`, the sidebar)
shows its box and then gives it the keyboard; if the focused tile goes away, the current session's tile (else the first)
is focused; the status pill and jump-to-latest
button are the same, scaled by position only.

Motion:

| What | Animation |
|---|---|
| Tile enters | opacity 0 → 1 and scale `tileEnterScale` 0.94 → 1, `Motion.standard`, `Motion.ease`; the slot exists at once, the others reflow at the same time |
| Tile leaves | opacity → 0 and scale → `tileExitScale` 0.96, `Motion.standard`, `Motion.exitEase`; the tile stays where it was (below the others) while it fades; the others already reflow into its slot |
| Reflow | `x`, `y`, `width`, `height` of every tile with `Motion.standard`, `Motion.ease` |
| Focus moves | border color `Motion.standard`; focus ring is only the border, nothing grows |
| Zoom in | the tile's frame animates to the content area's rect (`Motion.slow`, `Motion.ease`), the other tiles fade out and shrink to `tileExitScale` (`Motion.standard`), the header's height 36 → 0 and its opacity → 0 (`Motion.standard`), the pane's margins go from the compact to the full values. When it ends the single-session pane takes over with identical geometry: no visible swap |
| Zoom out | the reverse: the pane's frame animates from the content rect back to its slot (`Motion.slow`), the others fade in (`Motion.standard`) |
| Enter / leave split view | the current session's pane shrinks into its tile slot / grows out of it, as zoom; the other tiles fade in / out |

The zoomed tile is the shared element: it is the same `SessionPane` item during the animation, so scroll position,
streaming text and Hold survive. Hold (D5) is per pane and keeps working inside tiles.
Zoom is `Ctrl+Shift+Z` on the focused tile, a click on the maximize button, or a double click on the header.
Focus moves by clicking a tile, `Alt+arrows` (nearest tile in that direction by centre distance) or `Tab` outside
the composer. Typing goes to the focused tile's composer; a click on a tile focuses its composer.

## Command palette (Ctrl+P)

```
            ┌──────────────────────────────────────┐   width 560, top = max(48, 14% of the window height)
            │ ⌕  Type a command…                   │   52px search row
            ├──────────────────────────────────────┤   1px border
            │ ▣  New session          Ctrl+Shift+N │   rows 40px, at most 9 visible (scrolls)
            │ ▣  Switch session       Ctrl+Shift+L │
            └──────────────────────────────────────┘
```

- Scrim `Theme.scrim` over the whole window, fades with `Motion.standard`. A click on it closes.
- Panel: `Theme.surface`, 1px `border`, radius `radiusLarge` (18), clipped. Enter: opacity 0 → 1, y from −`dialogDrop`
  (16) to 0 and scale 0.97 → 1 (`popScale`), `Motion.standard`, `Motion.ease`. Leave: opacity → 0 only, `Motion.quick`.
- Search row: `search-16` (textMuted) at x = 18, text field `Theme.fontSize`, no border of its own.
- Rows: radius 8, inset 8px. `icon-16` (textMuted) at x = 12, title (`Theme.fontSize`, `text`), shortcut chips at the
  right (`smallFontSize`, textMuted, 1px `border`, radius 6, padding 6×2; one chip per key, 4px apart). Disabled
  commands: opacity 0.4, not selectable. A slash alias is shown after the title in textMuted (`/new`).
- One highlight rectangle (`surfaceHover`, radius 8) slides to the selected row, `y` with `Motion.quick`. Up/Down
  move it (wrapping), hover moves it, Enter / click runs the command, Esc closes.
- Empty query: commands grouped under category labels (28px rows, `smallFontSize`, textMuted). With a query: one
  flat list in `match()` order, no labels. Nothing matches: centred muted "No matching commands".
- Commands that take an argument (`rename`, `cd`, `export`, `variant`, `close-session`, `peers-name`): choosing one
  turns the search row into an argument prompt: a chip with the command title replaces the search icon, the
  placeholder is the command's hint, Enter runs it, Backspace on an empty field goes back to the list.
  The list below it is replaced by a one-line description (muted).

## Dialogs

Shared frame `ModalDialog.qml`: same scrim and enter / leave as the palette; panel `surface`, 1px `border`, radius
`radiusLarge`; width `dialogWidth` 480 (`dialogWidthWide` 640 for help and peers); max height 80% of the window,
content scrolls. Header 56px: title (`Theme.fontSize+1`, bold) at x = 22 and a 28px `close-20` face button at the
right (14px). Footer, when there are buttons: 64px, buttons right-aligned, 10px apart, 22px from the edge.
Esc closes; Enter triggers the primary button when the focus is not in a list. Opening a dialog while another is
open replaces it without re-running the scrim animation (the panels cross-fade, `Motion.quick`). On close the
focus returns to the composer of the focused session.

Buttons (`DialogButton.qml`): 34px high, radius 17, padding 18, text `Theme.fontSize-1`.
Primary: `accent` fill, `onAccent` text. Secondary: `surface` fill, `border` border, `text` text. Danger:
`danger` fill, `onAccent` text. Hover: secondary goes to `surfaceHover`, primary and danger to `Qt.lighter(fill, 1.08)`,
`Motion.quick`; press scales the face to 0.94. Disabled: opacity 0.4.

Text fields in dialogs: 38px high, radius 10, `background` fill, 1px `border` (`accent` focused, `Motion.standard`),
text `Theme.fontSize`, 12px padding. An inline error under a field: `smallFontSize`, `danger`, fades in
(`Motion.standard`), field border turns `danger`.

**Pick lists** (models, agents, variants, sessions): a search field on top (like the palette's but in the dialog,
52px) and rows 44px high (palette highlight behavior, the same sliding highlight). Optional section labels 28px.
The row of the current choice has `check-16` in `accent` at the right edge.
- Models: grouped by provider (label = provider name). Row: model name (`fontSize`), line 2 `smallFontSize` textMuted:
  `200k context · reasoning · images`. Choosing a model keeps the current variant when the new model has it.
- Agents: row: `agent-16`, name, description on line 2 (elided); a colored 8px dot at the left when the agent has a color.
- Variants: row = variant name; the current one is checked.
- Sessions: Live and History sections with the sidebar's row content (state dot, title, dirname); search over both;
  Enter opens; the live rows have the hover `close-16` button.

**MCP** (480): one row per server, 44px: status dot (8px, as in the footer; failed = `danger`), name, status text at
the right (textMuted; `danger` when failed), chevron (`Chevron`, rotates 90° with `Motion.standard`) when there is
an error or tools. Expanded: the error in `danger` (`codeFontSize`, mono, wrapped) and the tool names as a wrapped
list of chips (mono `smallFontSize`, `codeBackground` fill, radius 6, padding 6×2). Expanding animates the row
height with `Motion.standard`. "Refresh" secondary button in the footer. Empty: "No MCP servers configured".

**Peers** (640): a segmented control at the top (Peers / Name / Inbox / Outbox): 30px high, radius 15, `surface`
fill, `border` border, the indicator is a `userBubble` pill with `userBubbleBorder` that slides to the chosen segment
(`x`, `width` with `Motion.standard`). Below it a `codeBackground` box (radius 10, padding 14, 1px `border`) with the
backend's text in `Theme.monoFamily` at `codeFontSize`, selectable, scrolling, min height 200. Content cross-fades
(`Motion.quick`) when the tab changes or the result arrives; while loading the box shows "Loading…" in textMuted.
Name tab: a text field with the current peer name and a "Set name" primary button below the text.

**Help** (640): two sections, "Keyboard" and "Commands", each a table of rows 30px high (`smallFontSize+1`): left the
name, right the keys as chips (palette style); commands show `/name` aliases and the hint in textMuted. Generated from
the CommandRegistry, so it cannot drift from the real shortcuts. Fixed keys that are not commands (Esc Esc, Tab,
Enter, Shift+Enter) are listed in a "Writing" group.

**Export** (480): text "Write the conversation as markdown to this path on the backend's machine." (`smallFontSize`,
textMuted), a path field prefilled with `ref-session-<id>.md`, inline error, buttons Cancel / Export.
Success: the dialog closes and a toast shows the resolved path.

**Change directory** (480, `/cd`): path field prefilled with the session's directory, selected. Below it (remote
backend or typing) the completion list from `ListDirs`: rows 32px, `folder-16` + name, at most 6 visible; Tab or click
accepts a row (appends `/`), Up/Down select. On a local backend a secondary "Browse…" button in the footer opens the
`FolderDialog`. Apply shows the backend's rejection (e.g. "busy") inline. Buttons Cancel / Change.

**New session in a remote backend** (480; local backends use the native `FolderDialog` directly and never show this):
the same path field + completion list as `/cd`, title "New session", buttons Cancel / Start.

**Close session** (480): shown only when the session is busy or waiting. Body "This session is still working. Closing
it stops the work; the conversation stays in history." Buttons Cancel / Close (danger).

**Rename** (480): one text field with the name, buttons Cancel / Rename. Names are unique among live sessions: the
backend's rejection shows inline.

## Settings panel (Ctrl+Shift+S)

Dialog 520 wide, title "Settings". Sections are labels (`smallFontSize`, textMuted, 28px) with rows of 48px below,
label (`Theme.fontSize`) at the left, control at the right, 1px `border` dividers inset 22px.

| Row | Control |
|---|---|
| Theme | segmented control System / Light / Dark |
| Language | segmented control System / English / 简体中文 (the language names are not translated) |
| Notifications | switch (master) |
| …Permission requests, …Turn finished, …Turn failed | switches, indented 18px; dimmed to 0.4 and inactive while the master is off |

Footer line (not a row): "ref-gui 0.x · backend 0.x" in `smallFontSize` textMuted, selectable.
If notifications are unavailable on this system the master switch is replaced by textMuted text "Not available on
this system" and the sub-rows stay dimmed; the reason is shown below it in `smallFontSize`.

Segmented control (`Segmented.qml`, used by Settings and Peers): 30px high, radius 15, `surface` fill, `border`
border, segments equally wide, label `smallFontSize+1`; the chosen label is `Theme.text`, the others textMuted; the
indicator (`userBubble` fill, `userBubbleBorder` border, radius 13, inset 2px) slides with `x` / `width`
`Motion.standard` + `Motion.ease`. Clicking a theme segment runs the theme snapshot crossfade.

Switch (`ToggleSwitch.qml`): track 38×22, radius 11, `border` colored when off and `accent` when on (`Motion.quick`);
knob 16px, inset 3px, `textMuted` when off and `onAccent` when on; `x` moves with `Motion.quick` + `Motion.ease`, no
overshoot; while pressed the knob is 3px wider (`Motion.quick`).

## Toasts

Stack in the top right of the content area, below the top-right buttons: right margin 14, top margin 62, width up to
`toastWidth` 320, 8px apart, newest at the bottom of the stack, at most 4 (older ones leave early).

- Toast: radius `Theme.radius`, 1px border, padding 12×10. Icon 16px (`info-16` / `warning-16` / `error-16`) in
  `accent` / `warning` / `danger`, 10px gap, text `smallFontSize+1` `Theme.text`, wrapped to at most 4 lines, elided.
  Fill / border: info `surface` / `border`, warn `warningSurface` / `warningBorder`, error `errorSurface` / `errorBorder`.
- Enter: opacity 0 → 1 and x from +`toastSlide` (24) to 0, `Motion.standard` + `Motion.ease`; the icon plays
  `Flourish.enter()`. Leave: opacity → 0 and the item's height collapses, `Motion.standard` (the stack closes the gap).
- Lifetime: info 3.5 s, warn 6 s, error 8 s (`toastInfoMs`, `toastWarnMs`, `toastErrorMs`). Hovering pauses the
  timer. A click dismisses. A toast with the same text and level as one already shown restarts that one's timer and
  plays `Flourish.play()` on its icon instead of stacking.
- The status pill at the top center (connection trouble, last failure) stays as it is; toasts are for events.

## Backend banner

A full-width bar at the top of the content area, 40px, `warningSurface` fill with a 1px `warningBorder` line below.
Shown when the handshake reports a backend older than `ProtocolVersion`: `warning-16`, "The backend is older than this
app (protocol 1, needs 2).", and a `DialogButton` "Restart backend" (small: 28px high). The button is dimmed while sessions are
running or waiting, with the tooltip "Close or finish running sessions first". After the restart the banner slides up
(`y` and `opacity`, `Motion.standard`). The content area below moves with it (`Motion.standard`).

## Keyboard map (GUI)

One letter per command, shared with the WebUI. The GUI uses Ctrl+Shift+letter (Ctrl is Cmd on macOS); the WebUI uses
Alt+letter. Command ids are the TUI's canonical slash names (`/new`, `/sessions`, …). Titles are the TUI's `desc` texts,
except where the behavior differs.

| Key | Id | Title |
|---|---|---|
| Ctrl+P | `palette` | Command palette |
| Ctrl+Shift+N | `new` | New session (in the window's directory, at once) |
| (none) | `new-in` | New session in another folder… |
| Ctrl+Shift+L | `sessions` | Switch session (live + history) |
| Ctrl+Shift+M | `models` | Switch model (this session only) |
| Ctrl+Shift+A | `agents` | Switch agent |
| Ctrl+Shift+T | `variant` | Set or cycle model variant (no argument: next variant) |
| Ctrl+Shift+C | `compact` | Compact the conversation (runs at once, no confirmation) |
| Ctrl+Shift+U | `undo` | Undo last prompt |
| Ctrl+Shift+K | `close` | Close this session |
| Ctrl+Shift+X | `export` | Export transcript as markdown |
| Ctrl+Shift+Y | `copy` | Copy last assistant message (raw) |
| Ctrl+Shift+R | `thinking` | Toggle expanded thinking |
| Ctrl+Shift+O | `details` | Toggle expanded tool details |
| Ctrl+Shift+W | `watch` | Split view of background sessions |
| Ctrl+Shift+B | `sidebar` | Toggle sidebar |
| Ctrl+Shift+H | `help` | Keybindings and commands |
| Ctrl+Shift+S | `settings` | Settings |
| Ctrl+Shift+Z | `zoom` | Zoom the focused tile, or leave zoom (split view only) |
| Alt+arrows | (tile focus) | Move the focus between tiles (split view); not in the registry |

Palette-only (no key): `new-in`, `rename`, `cd`, `close-session`, `peers`, `peers-name`, `peers-inbox`,
`peers-outbox`, `mcp`. There are no `editor` and `exit` commands. Slash aliases are the TUI's (`/clear`, `/resume`,
`/continue`, `/effort`, `/summarize`, `/kill`, `/split`, `/cwd`, `/list-agents`, `/mcps`, …).
Owned by the composer (area C), not registry commands: Enter, Shift+Enter, Ctrl+J, Tab / Shift+Tab (cycle agents, via
`Workspace.cycleAgent`), Esc Esc (interrupt), ↑ / ↓ (prompt history), Ctrl+V. Unlike the TUI, the GUI does not bind
Ctrl+T: variants cycle with Ctrl+Shift+T or by clicking the variant pill.
Unverified: Ctrl+Shift+U is taken by IBus' Unicode input on Linux.

## As built (differences from the spec above)

- In split view the grid starts below the window buttons (split, theme, leave zoom) instead of under them.
- Zoom is left with `Ctrl+Shift+Z` (the `zoom` command toggles) or the restore button (the header is hidden while zoomed); `Esc` is not bound, it belongs to the input box, completions and permission questions. `Alt+arrows` pick the nearest tile by centre distance, across-distance counting twice.
- The folder picker (`FolderDialog`) starts in the platform's default directory.
- `Ctrl+Shift+Z` may be taken by the input box's redo (Unverified); the maximize button and the header double click work.
- The history list in the sidebar filters on the client, over the 200 most recent sessions the backend lists.

## New constants

`Theme.qml` (colors, `dark` / `light`): `warning` `#e6b450` / `#b7791f`; `warningSurface` `#2b2417` / `#fcf4e2`;
`warningBorder` `#5a4723` / `#ecd29a`; `scrim` `#8c000000` / `#40000000`.

`Theme.qml` (metrics, px): `sidebarWidth` 264, `sidebarPadding` 8, `sidebarHeaderHeight` 52, `sidebarRowHeight` 56,
`sidebarHistoryRowHeight` 44, `sidebarSectionHeight` 28, `sidebarFooterHeight` 44, `sidebarMinRunningRows` 3, `sidebarMinHistoryRows` 3,
`buttonHeight` 34, `fieldHeight` 38, `paletteWidth` 560, `paletteSearchHeight` 52, `paletteRowHeight` 40,
`paletteMaxRows` 9, `paletteTopRatio` 0.14, `dialogWidth` 480, `dialogWidthWide` 640, `settingsWidth` 520,
`dialogHeaderHeight` 56, `dialogFooterHeight` 64, `dialogMaxHeightRatio` 0.8, `tileMinWidth` 420,
`tileMinHeight` 300, `tileGap` 12, `tilePadding` 16, `tileHeaderHeight` 36, `toastWidth` 320, `toastMax` 4,
`bannerHeight` 40, `windowMinWidth` 640, `windowMinHeight` 420.

`Motion.qml`: `dialogDrop` 16 (px), `popScale` 0.97, `tileEnterScale` 0.94, `tileExitScale` 0.96, `toastSlide` 24 (px),
`toastInfoMs` 3500, `toastWarnMs` 6000, `toastErrorMs` 8000. No new durations or curves: everything else is
`quick` / `standard` / `slow`, `ease`, `exitEase`, `pulse`.

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
- Clicking the agent or model pill runs the `agents` / `models` command (their dialogs); clicking the variant pill
  opens the variants dialog. Cycling the variant is the `variant` command. Tooltips: "Agent (Tab)", "Model: <provider> /
  <model id>", "Variant".
- Morph when a value changes (Tab, Shift+Tab, the variant command, a dialog, `/model`, another client): the pill width
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
- Row, `/`: label `/name` (`Theme.text`, `Theme.fontSize`);
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
- Tokens: like the TUI, attaching an image inserts the token `[Image N]` at the cursor (a space before it
  when the cursor follows other text, one after it), N counting up per box. The prompt text is sent with the
  tokens, so history and restored drafts interchange with the TUI and the WebUI. Removing a thumbnail deletes its
  token from the text; deleting a token from the text removes the attachment. Images whose token is gone are not
  sent. A restored draft binds the images to its tokens in numeric order and renumbers them after the box's own;
  images without a token get one at the end.
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
  and the next Esc arms it again. A delayed tool call is cancelled by a single Esc (as in the TUI): the
  composer does it when the session's newest tool is delayed and no list is open and shell mode is off, and says so in a toast.
  Pending permission questions take Esc (deny) before the composer does; see the conversation area.

### Keys in the box

| Key | Effect |
|---|---|
| Enter | Send (or run the command / shell line). Not while an input method is composing. |
| Shift+Enter, Ctrl+J | New line |
| Tab / Shift+Tab | Next / previous agent (Tab accepts the list entry while a list is open) |
| Ctrl+Shift+T | Next variant of the model (the `variant` command; plain Ctrl+T is not bound) |
| Ctrl+V | Paste; an image on the clipboard becomes an attachment |
| Up / Down | Prompt history: Up when the box is empty, the cursor is at the very start, or history is being browsed; Down while browsing and on the last line. Otherwise they move the cursor. |
| Esc Esc | Interrupt the session |
| Esc | Close the list; leave shell mode |
| Ctrl+C | With a selection copy; otherwise clear the box (text of 20+ characters goes to the history first) |

### Notification settings

`NotificationSettingsSection.qml` is a block for the settings panel. Rows are 40 px high (`Theme.fontSize` label
left, switch right), `bell` icon (20, `Theme.textMuted`) in the section header "Notifications":

1. "Show system notifications" (master switch);
2. "A session waits for you (permission or question)", "A turn finished", "A turn ended with an error", indented 24 px, 40 % opacity
   and not clickable while the master switch is off (`Motion.standard`). Other sessions' messages never notify.

When the system cannot show notifications the block adds a note under the master switch in
`Theme.smallFontSize` / `Theme.textMuted`: "Not available on this system. The taskbar flashes instead." (the
reason from the platform layer is the tooltip). The switches still work: they choose when the flashing happens.

### New visual constants

- `Theme.qml`: `shellSurface`, `shellBorder`, `agentOrange`, `agentBlue`, `agentPurple`, `agentRed`,
  `agentYellow`, `agentGreen`, `agentCyan`, function `agentColor(spec, index)`.
- `Motion.qml`: `interruptWindow` (1500).
- Component literals, at the top of each file: `Composer.qml` `padding` 8, `maxInputHeight` 220, `tagRowHeight` 32,
  `textGap` 4, `queueGap` 8, `inputLeftInset` 4, `thumbRowGap` 8; `ComposerChip.qml` `chipHeight` 28, `paddingX` 10,
  `iconGap` 6, `labelSlide` 6, `maxWidth` 240; `ComposerTags.qml` `chipGap` 4, `hintGap` 12; `AttachmentStrip.qml`
  `thumbSize` 56, `thumbGap` 8, `attachmentRemoveSize` 18, `removeBackgroundOpacity` 0.85, `enterScale` 0.8;
  `CompletionPopup.qml` `popupGap` 8, `popupPadding` 4, `popupRowHeight` 36, `popupMaxRows` 8; `QueueStrip.qml`
  `queueRowHeight` 32, `queueMaxRows` 4, `removingOpacity` 0.5; `NotificationSettingsSection.qml` `rowHeight` 40,
  `subIndent` 24, `disabledOpacity` 0.4.

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
| `info` | #2a7fb5 | #6fb7e0 | messages from other sessions, hunk headers, tool input |
| `diffAdded` / `diffAddedBackground` | #1f7a4d / #e3f3e9 | #6cc28a / #1d2f27 | added lines |
| `diffRemoved` / `diffRemovedBackground` | #b3303f / #fbe6e8 | #ec6f73 / #33202a | removed lines |
| `matchBackground` | #fff1c2 | #3a3420 | grep match lines |

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
  Style: 1 border in the colour at 50%, icon in the colour, label in `Theme.text`, fill = colour at 12% on hover and 22% pressed.
  Key cap: 18 x 18, radius 4, 1 border in the same colour, mono 11 letter in the colour.
- Right end: "+N more waiting" 12 `textMuted` when other requests of this session are pending.
- Keys act on the oldest pending request of the session: `a` or Enter allow, `s` always, `d` or Esc deny. Keys are ignored for
  `permissionGuard` = 350 ms after the request appears (the user was still typing); letters then no longer reach the composer
  until all requests are answered. As in the TUI and the WebUI the keys work with the caret in the input box: the box offers
  every key to `ChatSession::permissionKey` first (`Composer.keyOffered`). It answers, swallows other printable keys, and
  leaves Ctrl/Alt/Meta shortcuts and editing keys (Backspace, arrows) alone. Shifted letters do not answer, a held key
  answers once. In split view only the focused (or zoomed) pane offers its keys.
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
Bubbles keep the human bubble's radius and padding; the header sits 6 above the text. The bar is 3 wide, rounded, at the left padding (14) and inset
by the vertical padding top and bottom; the text and the header start 10 right of the bar.

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

## Removed rows

A row that goes away (Undo, a history rewritten by compaction) fades out over `Motion.quick`, then its height closes over
`Motion.standard`; the rows below follow. Rows that did not change stay where they are: the history is compared row by row and
only the differing tail is removed and added again. Rows added this way enter like new ones; earlier history loaded by paging
does not animate.

## Constants added (for the WebUI's list)

Theme: `info`, `diffAdded`, `diffAddedBackground`, `diffRemoved`, `diffRemovedBackground`, `matchBackground`; number `staleOpacity`. `warning` and the agent colours (`Theme.agentColor`) are the shell's and composer's.
Component literals: `permissionGuard` 350 ms, `previewLines` 200, `lightboxMargin` 48, `olderTriggerDistance` 400, `shimmerPeriod` 1600,
`retainedDistance` 20000 (ChatView: px of rows kept instantiated around the view), `thumbnailMaxWidth` 160, `thumbnailMaxHeight` 96, `userThumbnailHeight` 72, `liveTailLines` 20, `spreadSpeed` 1.5.
