#!/usr/bin/env python3
"""Build every program-icon file from the SVG sources in assets/app/.

Sources (hand-drawn):
  ref.svg              1024 units, full-bleed tile. Used from 64px up.
  ref-small.svg        32 units, flat and heavier, every coordinate even so that 16, 32 and 48px renders
                       fall on whole pixels. Used up to 48px, for the favicon and the tab icon.
  ref-small-alert.svg  ref-small.svg with a red dot: the browser tab icon while a session needs attention.

Outputs (committed):
  png/ref-<size>.png          full-bleed, for Windows, Linux and the window icon.
  ref.ico                     Windows: 16-256, each size rendered on its own (not resampled).
  ref-macos.svg, ref.icns     macOS: body inset on the canvas, with a shadow (see MACOS_*).
  web/favicon.svg|png, web/favicon-alert.svg|png

Usage: build_app_icons.py
"""

import io
import shutil
import struct
from pathlib import Path

import resvg_py
from PIL import Image

APP_DIR = Path(__file__).resolve().parent.parent / "app"
LARGE_SVG = APP_DIR / "ref.svg"
SMALL_SVG = APP_DIR / "ref-small.svg"
SMALL_ALERT_SVG = APP_DIR / "ref-small-alert.svg"

LARGE_UNITS = 1024
SMALL_UNITS = 32
SMALL_MAX_PIXELS = 48  # largest size drawn from the small master

PNG_SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
FAVICON_SIZE = 32

# macOS app icon grid: 1024 canvas, 824 body, 100 transparent margin per side. Apple's HIG and the
# Production Template in Apple Design Resources are the reference; the 824 value was measured on shipped
# Apple and third-party icons (github.com/alexey-pelykh/sessiometer/issues/952). Corner radius follows
# the artwork: 230 * 824/1024 = 185, Apple's template says 185.4.
# The shadow (28px blur, 12px down, black) is the commonly used template value. Unverified against Apple's file.
MACOS_CANVAS = 1024
MACOS_BODY = 824
MACOS_SHADOW_BLUR = 28
MACOS_SHADOW_OFFSET_Y = 12
MACOS_SHADOW_OPACITY = 0.3

# (icns type, pixel size). All entries hold PNG data. "@2x" types repeat the pixel size of the next size down.
ICNS_ENTRIES = (
    (b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024),
    (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512),
)


def render(svg_text: str, pixels: int) -> bytes:
    return resvg_py.svg_to_bytes(svg_string=svg_text, width=pixels, height=pixels)


def source_for(pixels: int) -> tuple[str, int]:
    """SVG text and its unit size for a given output size."""
    path = SMALL_SVG if pixels <= SMALL_MAX_PIXELS else LARGE_SVG
    return path.read_text(encoding="utf-8"), SMALL_UNITS if path == SMALL_SVG else LARGE_UNITS


def svg_body(svg_text: str) -> str:
    start = svg_text.index(">", svg_text.index("<svg")) + 1
    return svg_text[start:svg_text.rindex("</svg>")]


def macos_svg(svg_text: str, units: int) -> str:
    """The icon inset on the macOS grid, with the template's shadow."""
    margin = (MACOS_CANVAS - MACOS_BODY) / 2
    scale = MACOS_BODY / units
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{MACOS_CANVAS}" height="{MACOS_CANVAS}" viewBox="0 0 {MACOS_CANVAS} {MACOS_CANVAS}">
  <defs>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="150%">
      <feDropShadow dx="0" dy="{MACOS_SHADOW_OFFSET_Y}" stdDeviation="{MACOS_SHADOW_BLUR / 2}" flood-color="#000000" flood-opacity="{MACOS_SHADOW_OPACITY}"/>
    </filter>
  </defs>
  <g filter="url(#shadow)">
    <g transform="translate({margin:g} {margin:g}) scale({scale:.10g})">{svg_body(svg_text)}</g>
  </g>
</svg>
"""


def macos_png(pixels: int) -> bytes:
    text, units = source_for(pixels)
    return render(macos_svg(text, units), pixels)


def write_png_set() -> None:
    folder = APP_DIR / "png"
    folder.mkdir(exist_ok=True)
    for pixels in PNG_SIZES:
        text, _ = source_for(pixels)
        (folder / f"ref-{pixels}.png").write_bytes(render(text, pixels))


def write_ico() -> None:
    """ICO with PNG-compressed entries. Pillow would resample one image, so write the container directly."""
    images = [(pixels, render(source_for(pixels)[0], pixels)) for pixels in ICO_SIZES]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries, data = b"", b""
    for pixels, png in images:
        dimension = 0 if pixels >= 256 else pixels  # 0 means 256
        entries += struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    (APP_DIR / "ref.ico").write_bytes(header + entries + data)


def write_macos() -> None:
    (APP_DIR / "ref-macos.svg").write_text(macos_svg(LARGE_SVG.read_text(encoding="utf-8"), LARGE_UNITS), encoding="utf-8")

    chunks = b""
    for kind, pixels in ICNS_ENTRIES:
        png = macos_png(pixels)
        chunks += kind + struct.pack(">I", 8 + len(png)) + png
    (APP_DIR / "ref.icns").write_bytes(b"icns" + struct.pack(">I", 8 + len(chunks)) + chunks)


def write_favicons() -> None:
    folder = APP_DIR / "web"
    folder.mkdir(exist_ok=True)
    for source, name in ((SMALL_SVG, "favicon"), (SMALL_ALERT_SVG, "favicon-alert")):
        shutil.copyfile(source, folder / f"{name}.svg")
        (folder / f"{name}.png").write_bytes(render(source.read_text(encoding="utf-8"), FAVICON_SIZE))


def verify() -> None:
    """Read every container back: sizes in the ICO, entries in the ICNS, canvas of the PNGs."""
    with Image.open(APP_DIR / "ref.ico") as ico:
        assert set(ico.info["sizes"]) == {(s, s) for s in ICO_SIZES}, ico.info["sizes"]
    with Image.open(APP_DIR / "ref.icns") as icns:
        assert icns.size == (MACOS_CANVAS, MACOS_CANVAS), icns.size
    for pixels in PNG_SIZES:
        with Image.open(APP_DIR / "png" / f"ref-{pixels}.png") as png:
            assert png.size == (pixels, pixels)


def main() -> None:
    write_png_set()
    write_ico()
    write_macos()
    write_favicons()
    verify()
    print("ok")


if __name__ == "__main__":
    main()
