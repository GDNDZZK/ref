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
         56×n session rows (list scrolls beyond max(half of the body, 3 rows))
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

State dot (8px, same drawing as `StatusDot`):

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

Replaces the SessionPane in the content area; the sidebar stays and highlights the tiles' sessions with a 2px
`accent` bar at the row's left edge (`isTile`). Tiles are the live sessions nobody is attached to plus the current
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
tile width − 2 × 16; the composer's outer margin is 10 instead of `pagePadding`; the status pill and jump-to-latest
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
Zoom is `Ctrl+O` on the focused tile, a click on the maximize button, or a double click on the header.
Focus moves by clicking a tile, `Alt+arrows` (nearest tile in that direction by centre distance) or `Tab` outside
the composer. Typing goes to the focused tile's composer; a click on a tile focuses its composer.

## Command palette (Ctrl+P)

```
            ┌──────────────────────────────────────┐   width 560, top = max(48, 14% of the window height)
            │ ⌕  Type a command…                   │   52px search row
            ├──────────────────────────────────────┤   1px border
            │ ▣  New session                Ctrl+N │   rows 40px, at most 9 visible (scrolls)
            │ ▣  Sessions                   Ctrl+K │
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
Ctrl+T, Enter, Shift+Enter) are listed in a "Writing" group.

**Export** (480): text "Write the conversation as markdown to this path on the backend's machine." (`smallFontSize`,
textMuted), a path field prefilled with `ref-session-<id>.md`, inline error, buttons Cancel / Export.
Success: the dialog closes and a toast shows the resolved path.

**Compact** (480): body "Summarize the older part of the conversation to free up context. This asks the model and
cannot be undone." and a context bar: 6px high, radius 3, `border` track, `accent` fill with width
`used / max` (`Motion.standard`), caption "84k of 200k tokens (42%)" in `smallFontSize` textMuted; a tick mark 2px wide
in `textMuted` at `recommend / max`. Buttons Cancel / Compact (primary).

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

## Settings panel (Ctrl+,)

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

Switch (`Switch.qml`): track 38×22, radius 11, `border` colored when off and `accent` when on (`Motion.quick`);
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

| Key | Action | Command id |
|---|---|---|
| Ctrl+P | Command palette | `palette.open` |
| Ctrl+N | New session in the window's directory | `session.new` |
| Ctrl+Shift+N | New session in another folder… | `session.newIn` |
| Ctrl+K | Sessions (switch, live and history) | `session.list` |
| Ctrl+W | Close this session | `session.close` |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous live session | `session.next` / `session.previous` |
| Alt+1 … Alt+9 | Nth live session | `session.nth` |
| Ctrl+M | Model for this session | `session.model` |
| Ctrl+Shift+A | Agent | `session.agent` |
| Ctrl+Shift+E | Export transcript | `session.export` |
| Ctrl+Shift+C | Copy last reply | `session.copy` |
| Ctrl+Shift+R | Expand / collapse reasoning | `view.reasoning` |
| Ctrl+Shift+D | Expand / collapse tool details | `view.details` |
| Ctrl+B | Show / hide the sidebar | `view.sidebar` |
| Ctrl+\\ | Split view | `view.split` |
| Ctrl+O | Zoom the focused tile (split view) | `split.zoom` |
| Alt+arrows | Move the tile focus (split view) | `split.focus*` |
| Ctrl+, | Settings | `app.settings` |
| F1 | Help | `app.help` |
| Ctrl+Q | Quit | `app.quit` |

Owned by the composer (area C), not listed in the registry: Enter, Shift+Enter, Tab / Shift+Tab (agents), Ctrl+T
(variants), Esc Esc (interrupt), ↑ / ↓ (prompt history), Ctrl+V. Palette-only (no shortcut): compact, undo, rename, cd,
variant, peers*, mcp, close-session. The WebUI replaces Ctrl+N / Ctrl+T / Ctrl+W (browser-reserved) with Alt+N and
Alt+W; the rest is the same.

## New constants

`Theme.qml` (colors, `dark` / `light`): `warning` `#e6b450` / `#b7791f`; `warningSurface` `#2b2417` / `#fcf4e2`;
`warningBorder` `#5a4723` / `#ecd29a`; `scrim` `#8c000000` / `#40000000`.

`Theme.qml` (metrics, px): `sidebarWidth` 264, `sidebarPadding` 8, `sidebarHeaderHeight` 52, `sidebarRowHeight` 56,
`sidebarHistoryRowHeight` 44, `sidebarSectionHeight` 28, `sidebarFooterHeight` 44, `sidebarMinRunningRows` 3,
`buttonHeight` 34, `fieldHeight` 38, `paletteWidth` 560, `paletteSearchHeight` 52, `paletteRowHeight` 40,
`paletteMaxRows` 9, `paletteTopRatio` 0.14, `dialogWidth` 480, `dialogWidthWide` 640, `settingsWidth` 520,
`dialogHeaderHeight` 56, `dialogFooterHeight` 64, `dialogMaxHeightRatio` 0.8, `tileMinWidth` 420,
`tileMinHeight` 300, `tileGap` 12, `tilePadding` 16, `tileHeaderHeight` 36, `toastWidth` 320, `toastMax` 4,
`bannerHeight` 40, `windowMinWidth` 640, `windowMinHeight` 420.

`Motion.qml`: `dialogDrop` 16 (px), `popScale` 0.97, `tileEnterScale` 0.94, `tileExitScale` 0.96, `toastSlide` 24 (px),
`toastInfoMs` 3500, `toastWarnMs` 6000, `toastErrorMs` 8000. No new durations or curves: everything else is
`quick` / `standard` / `slow`, `ease`, `exitEase`, `pulse`.
