# assets

Hand-drawn artwork shared by the TUI, GUI and WebUI.

| Path | Content |
|---|---|
| `icons/` | SVG interface icons, two sets (16 and 20 grid). Start with [`icons/INDEX.md`](icons/INDEX.md). |
| `app/ref.svg` | Program icon, large mark (1024 units): four panes with r, e, f and the blue dot. From 64px up, macOS, Linux. |
| `app/ref-small.svg` | Program icon for 16-48px and the favicon: "r." with every straight edge on whole pixels. |
| `app/ref-small-alert.svg` | The small mark with a red dot: tab icon while a session needs attention. |
| `app/ref-lockup-dark.svg`, `app/ref-lockup-light.svg` | The large mark and the word "ref" (README header, title cards). |
| `app/ref.ico` | Windows: 16, 24, 32, 48, 64, 128, 256, each size rendered separately. |
| `app/ref.icns`, `app/ref-macos.svg` | macOS: 824px body on a 1024 canvas with the template shadow. |
| `app/web/` | `favicon.svg`, `favicon-alert.svg`, `favicon.png` (32px, for browser notifications). |
| `scripts/` | Lint, preview and build scripts. |

Everything under `app/` is generated from `scripts/logo.py` by `scripts/build_app_icons.py` and committed.
The GUI compiles `ref.svg` and `ref-small.svg` into the program for its window icon on Linux
(`gui/src/icons/app_icon.cpp`), so a change to them needs a rebuild of the GUI.

## Scripts

```bash
python3 -m venv assets/.venv
assets/.venv/bin/pip install -r assets/scripts/requirements.txt

assets/.venv/bin/python assets/scripts/check_icons.py       # lint icons/*.svg against the grid rules
assets/.venv/bin/python assets/scripts/render_previews.py   # icons/preview/*.png (1x, 1.5x, 2x, enlarged)
assets/.venv/bin/python assets/scripts/build_app_icons.py   # app/: SVG sources, ico, icns, favicons
```

Rendering uses [resvg](https://github.com/RazrFalcon/resvg) (`resvg-py`), which follows the SVG spec the way
browsers do. No system libraries are needed.

## Program icon

The large mark is ref's split view: a dark tile with four panes holding r, e, f and the accent-blue dot
(`Theme.qml` colours). The small mark keeps the r and the dot. Both use one monoline letter set whose
stroke matches the interface icons. The geometry and its rules (margins, concentric corners, optical
centring of the letters) are documented in `scripts/logo.py`.

macOS grid: 1024 canvas, 824 body (100px margin per side), corner radius 185 (Apple's template: 185.4),
shadow 28px blur, 12px down. The 824 and 185.4 values come from Apple's template as measured on shipped icons
(see the comment in `build_app_icons.py`); the shadow values are the commonly used ones. Unverified against Apple's
template file. `ref.icns` holds PNG entries only (`icp4`-`icp6`, `ic07`-`ic14`).
