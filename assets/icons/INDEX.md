# Icons

Hand-drawn, single-colour line icons shared by the GUI (Qt Quick) and the WebUI (Vue).
The look follows qutie's drawn glyphs (`gui/src/app/Chevron.qml`, `SendButton.qml`): round caps, round joins,
one stroke weight per size, no fills, no gradients.

## Two sets

Each icon exists in the sets that make sense for its use. Set is part of the file name: `<name>-16.svg`,
`<name>-20.svg`. The 16 set is redrawn, not scaled down: it has less detail.

| Set | Grid | Stroke | Use | Matches |
|---|---|---|---|---|
| `-20` | 20x20 | 2 | Buttons and toolbars: the glyph in a 32-34px round or square button. | qutie's 16px chevron with stroke 2 in `SendButton` / jump-to-latest |
| `-16` | 16x16 | 1.5 | Next to 12-13px text: tool-card titles, sidebar rows, `@` list items, the actions under a message, permission buttons. | qutie's inline 12px chevron with stroke 1.5 |

Always show an icon at its grid size in logical pixels (20 or 16). Do not scale: the pixel alignment below only
holds at that size.

## Pixel alignment

Strokes are placed so that their edges fall on whole device pixels.

| Set | Stroke centre lines at | 1x | 2x | 1.5x | 1.25x |
|---|---|---|---|---|---|
| `-20` | integers | crisp (2px) | crisp (4px) | straight lines on odd coordinates crisp (3px), others half a pixel soft | soft (2.5px) |
| `-16` | .25 and .75, mirrored pairs sum to 16 | soft (1.5px, like qutie's inline chevron) | crisp (3px) | soft | soft |

- Rectangles, tangent points of circles and arc ends sit on those coordinates. Diagonals are 45 degrees (or 2:1),
  so they are as regular as a diagonal can be.
- A curve's extreme that ends no straight edge (the bulge of the globe's meridian) may be up to 0.2 off the grid.
- 1.5x is a trade-off: icons are centred first, alignment second. 1.25x cannot be crisp for a 1.5px or 2px stroke.
- `assets/scripts/check_icons.py` enforces all of this (`assets/.venv/bin/python assets/scripts/check_icons.py`).
- `preview/icons-<set>-<scale>x.png` show the exact device pixels, enlarged (nearest neighbour), for the light
  theme's text colour. Regenerate with `assets/scripts/render_previews.py`.

## Colour

Files use `currentColor` only. No other colours, no opacity.

- **WebUI**: either inline the file (`import svg from "…/chat-16.svg?raw"`), where `currentColor` follows CSS `color`,
  or use it as a mask, which keeps pixel alignment and costs no inline markup:

  ```css
  .icon { width: 16px; height: 16px; background: currentColor;
          mask: var(--icon) center / 16px 16px no-repeat; }   /* --icon: url(…/chat-16.svg) */
  ```

  Size the element to the set (16 or 20 CSS px) and place it on whole CSS pixels (no fractional margins).
- **GUI**: Qt's `Image` does not apply `currentColor` (the glyph turns black). Load the SVG text, replace
  `currentColor` with the theme colour (`Theme.text`, `textMuted`, `accent`, `danger`, ...) and rasterise at
  `sourceSize = logicalSize * Screen.devicePixelRatio`, with the item at the logical size
  (an image provider does this in one place). Colourising a rendered white image (`MultiEffect`) works too but
  resamples. Unverified on Qt 6.12.

## Not here: stays drawn in QML

qutie draws these in QML, with animation, and the SVG set does not repeat them:

| Glyph | QML item |
|---|---|
| Expand / collapse arrow, jump-to-latest arrow (chevron, rotated) | `Chevron.qml` |
| Sun / moon theme toggle | `ThemeToggle.qml` |
| Send arrow, stop square | `SendButton.qml` |
| Status dots: session state, tool state (running pulse, done, failed, denied, cancelled) | `StatusDot.qml` |
| Alert badge ("!" in a red disc) on the sidebar, jump-to-latest and split tiles | `AlertBadge.qml` |
| Delay countdown (shrinking ring) | drawn in QML |
| Pulse, completion flourish, stream caret | `Pulse.qml`, `Flourish.qml`, `StreamCaret.qml` |

Deny (a permission) uses `close`, allow uses `check`, allow for the session uses `check-all`.

## Icons

Sets: `20`, `16` or both. Names are the file names without the set suffix.

### Layout and sessions

| Name | Sets | Meaning | Used for |
|---|---|---|---|
| `plus` | 20 | Add | New session |
| `chat` | 16, 20 | Speech bubble | A session; the `read_session` tool |
| `history` | 16, 20 | Clock with a counter-clockwise arrow | History sessions |
| `search` | 16, 20 | Magnifier | Search field; the `grep` tool |
| `close` | 16, 20 | Cross | Close, clear, deny a permission |
| `sidebar` | 20 | Window with a left panel | Collapse / expand the sidebar |
| `split` | 20 | Four tiles | Split view |
| `maximize` | 20 | Corners pointing outwards | Enlarge a split tile |
| `restore` | 20 | Corners pointing inwards | Back to the split view |
| `folder` | 16, 20 | Closed folder | A directory in lists and completion |
| `folder-open` | 20 | Open folder | Choose the session directory |
| `command` | 20 | Command key | Command palette |
| `sliders` | 20 | Three sliders | Settings |
| `bell` | 20 | Bell | Notification settings |

### Composer and messages

| Name | Sets | Meaning | Used for |
|---|---|---|---|
| `paperclip` | 20 | Paperclip | Attach a file |
| `image` | 16, 20 | Picture | Image attachment; image tool results |
| `at-sign` | 16 | @ | `@` file picker header |
| `slash` | 16 | / | `/` command picker header |
| `terminal` | 16, 20 | Prompt and cursor | `!` shell mode; the `bash` tool |
| `agent` | 16 | Robot head | Agent tag |
| `model` | 16 | Chip | Model tag |
| `variant` | 16 | Three rising bars | Variant (effort) tag |
| `copy` | 16, 20 | Two sheets | Copy a message or code block |
| `check` | 16, 20 | Tick | Copied; allow a permission |
| `undo` | 16, 20 | Curved arrow back | Undo (revert) a message |
| `recall` | 16 | Return arrow | Take a queued message back into the input |

### Tool types (tool-card titles)

| Name | Sets | Meaning | Tool |
|---|---|---|---|
| `file-text` | 16 | Sheet with lines | `read` |
| `pencil` | 16 | Pencil | `edit` |
| `file-plus` | 16 | Sheet with a plus | `write` |
| `trash` | 16 | Bin | `delete` |
| `terminal` | 16 | Prompt | `bash` |
| `search` | 16 | Magnifier | `grep` |
| `folder-search` | 16 | Folder with a magnifier | `find` |
| `globe` | 16 | Globe | `webfetch` |
| `todo` | 16 | Checklist | `todo` |
| `chat-plus` | 16 | Bubble with a plus | `create_session` |
| `chat-send` | 16 | Bubble with an arrow | `send_message`, `message_status` |
| `chat-list` | 16 | Bubble with lines | `list_sessions` |
| `chat-x` | 16 | Bubble with a cross | `close_session` |
| `chat` | 16 | Bubble | `read_session` |
| `mcp` | 16, 20 | Plug | MCP servers and MCP tools |
| `tool` | 16 | Hammer | Any other tool |
| `file` | 16 | Sheet | A file in the `@` list |

### Permission, status and information

| Name | Sets | Meaning | Used for |
|---|---|---|---|
| `lock` | 16 | Padlock | Waiting for permission |
| `check` | 16 | Tick | Allow |
| `check-all` | 16 | Double tick | Allow for the whole session |
| `close` | 16 | Cross | Deny |
| `clock` | 16 | Clock | Duration, delay |
| `retry` | 16 | Circular arrow | Retrying after an error |
| `error` | 16 | "!" in a circle | Error text |
| `warning` | 16 | "!" in a triangle | Warning text |
| `info` | 16 | "i" in a circle | Hint text |
| `question` | 16 | "?" in a circle | Waiting for your answer (a question from the agent) |
| `external-link` | 16, 20 | Box with an arrow | Open in browser / external viewer |
