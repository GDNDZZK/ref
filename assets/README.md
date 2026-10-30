# assets

Hand-drawn artwork shared by the TUI, GUI and WebUI.

| Path | Content |
|---|---|
| `icons/` | SVG interface icons, two sets (16 and 20 grid). Start with [`icons/INDEX.md`](icons/INDEX.md). |
| `app/ref.svg` | Program icon, large master (1024 units, full-bleed tile). |
| `app/ref-small.svg` | Program icon for 16-48px: flatter, heavier, pixel-aligned. |
| `app/ref-small-alert.svg` | Small master with a red dot: tab icon while a session needs attention. |
| `app/png/ref-<size>.png` | 16, 24, 32, 48, 64, 128, 256, 512, 1024. Full-bleed: Linux, Windows, window icons. |
| `app/ref.ico` | Windows: 16, 24, 32, 48, 64, 128, 256, each size rendered separately. |
| `app/ref.icns`, `app/ref-macos.svg` | macOS: 824px body on a 1024 canvas with the template shadow. |
| `app/web/` | `favicon.svg`, `favicon.png` (32px), `favicon-alert.svg`, `favicon-alert.png`. |
| `scripts/` | Lint, preview and build scripts. |

Everything under `app/png`, `app/web`, `app/ref.ico`, `app/ref.icns` and `app/ref-macos.svg` is generated and committed.

## Scripts

```bash
python3 -m venv assets/.venv
assets/.venv/bin/pip install -r assets/scripts/requirements.txt

assets/.venv/bin/python assets/scripts/check_icons.py       # lint icons/*.svg against the grid rules
assets/.venv/bin/python assets/scripts/render_previews.py   # icons/preview/*.png (1x, 1.5x, 2x, enlarged)
assets/.venv/bin/python assets/scripts/build_app_icons.py   # app/ png, ico, icns, favicons
```

Rendering uses [resvg](https://github.com/RazrFalcon/resvg) (`resvg-py`), which follows the SVG spec the way
browsers do. No system libraries are needed.

## Program icon

A dark tile with qutie's rounded chevron prompt and the accent-blue dot of its streaming caret
(`Theme.qml` colours: tile `#16171a` to `#25262b`, chevron `#e6e6e9`, dot `#7aa2f7`).
The small master replaces the large one up to 48px, so small sizes keep a heavy stroke and whole-pixel edges.

macOS grid: 1024 canvas, 824 body (100px margin per side), corner radius 185 (Apple's template: 185.4),
shadow 28px blur, 12px down. The 824 and 185.4 values come from Apple's template as measured on shipped icons
(see the comment in `build_app_icons.py`); the shadow values are the commonly used ones. Unverified against Apple's
template file. `ref.icns` holds PNG entries only (`icp4`-`icp6`, `ic07`-`ic14`).
