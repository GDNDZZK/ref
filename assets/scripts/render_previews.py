#!/usr/bin/env python3
"""Rasterise every icon at 1x, 1.5x and 2x and write zoomed contact sheets to assets/icons/preview/.

Each cell shows the exact device pixels (nearest-neighbour zoom), so pixel alignment can be
checked by eye. Rendering uses resvg, the same geometry rules as browsers. Colours are the
light theme's `text` on `background` from gui/src/app/Theme.qml.

Usage: render_previews.py
"""

import io
import re
from pathlib import Path

import resvg_py
from PIL import Image, ImageDraw

ICON_DIR = Path(__file__).resolve().parent.parent / "icons"
PREVIEW_DIR = ICON_DIR / "preview"

INK = "#1d1d1f"
PAPER = "#fafaf9"
GUTTER = "#c8c8c4"
LABEL_HEIGHT = 14
CELL_PADDING = 4
COLUMNS = 8
SETS = (16, 20)
SOLID_VALUES = {round(0.299 * 0x1D + 0.587 * 0x1D + 0.114 * 0x1F), 0x1D, 0x1E}  # INK in greyscale

# device scale -> nearest-neighbour zoom of the sheet, chosen so a cell is about the same size
SCALES = {1.0: 8, 1.5: 5, 2.0: 4}


def rasterise(svg_text: str, device_size: int) -> Image.Image:
    coloured = svg_text.replace("currentColor", INK)
    png = resvg_py.svg_to_bytes(svg_string=coloured, width=device_size, height=device_size, background=PAPER)
    return Image.open(io.BytesIO(png)).convert("RGB")


def soft_pixel_share(files: list[Path], device_size: int) -> float:
    """Share of inked pixels that are neither background nor full ink: edges that miss the pixel grid."""
    inked = soft = 0
    for file in files:
        pixels = rasterise(file.read_text(encoding="utf-8"), device_size).convert("L")
        paper = pixels.getpixel((0, 0))
        for value in pixels.get_flattened_data():
            if value == paper:
                continue
            inked += 1
            soft += value not in SOLID_VALUES
    return soft / inked


def build_sheet(size: int, scale: float, zoom: int) -> Image.Image | None:
    files = sorted(ICON_DIR.glob(f"*-{size}.svg"))
    if not files:
        return None

    device_size = round(size * scale)
    cell_width = device_size * zoom + 2 * CELL_PADDING
    cell_height = cell_width + LABEL_HEIGHT
    rows = -(-len(files) // COLUMNS)
    sheet = Image.new("RGB", (cell_width * COLUMNS, cell_height * rows), GUTTER)
    draw = ImageDraw.Draw(sheet)

    for index, file in enumerate(files):
        column, row = index % COLUMNS, index // COLUMNS
        left, top = column * cell_width, row * cell_height
        glyph = rasterise(file.read_text(encoding="utf-8"), device_size)
        glyph = glyph.resize((device_size * zoom, device_size * zoom), Image.NEAREST)
        sheet.paste(glyph, (left + CELL_PADDING, top + CELL_PADDING))
        draw.text((left + CELL_PADDING, top + cell_width - 1), re.sub(rf"-{size}$", "", file.stem), fill="#000")
    return sheet


def main() -> None:
    PREVIEW_DIR.mkdir(exist_ok=True)
    for size in SETS:
        for scale, zoom in SCALES.items():
            sheet = build_sheet(size, scale, zoom)
            if sheet is None:
                continue
            name = f"icons-{size}-{scale:g}x.png"
            sheet.save(PREVIEW_DIR / name, optimize=True)
            share = soft_pixel_share(sorted(ICON_DIR.glob(f"*-{size}.svg")), round(size * scale))
            print(f"{name}  {sheet.size}  soft pixels {share:.0%}")


if __name__ == "__main__":
    main()
