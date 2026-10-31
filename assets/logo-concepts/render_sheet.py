#!/usr/bin/env python3
"""Write one contact sheet per logo concept, plus its SVG sources, into assets/logo-concepts/<key>/.

Each sheet: name and idea; 1024 / 256 / 32 / 16 px on dark and on light; the alert variant and browser
tabs; the lockup on dark and light; a macOS dock and a Windows taskbar next to generic app icons.

Usage: render_sheet.py   (run with assets/.venv/bin/python)
"""

import io
import sys
from pathlib import Path

import resvg_py
from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "scripts"))

import build_app_icons  # noqa: E402  (macOS grid wrapper)
import concepts  # noqa: E402
import neighbours  # noqa: E402

FONT_FILE = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"
FONT_BOLD_FILE = "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc"
FONT_INDEX_SC = 2

WIDTH = 2000
MARGIN = 48
DARK_BG = "#1b1c1f"
LIGHT_BG = "#f4f4f2"
SHEET_BG = "#ffffff"
LABEL_ON_DARK = "#a0a2aa"
LABEL_ON_LIGHT = "#6b6d75"
SMALL_MAX = 48


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(FONT_BOLD_FILE if bold else FONT_FILE, size, index=FONT_INDEX_SC)


def render(svg_text: str, width: int, height: int | None = None) -> Image.Image:
    png = resvg_py.svg_to_bytes(svg_string=svg_text, width=width, height=height or width)
    return Image.open(io.BytesIO(png)).convert("RGBA")


def icon(concept: concepts.Concept, pixels: int, alert: bool = False) -> Image.Image:
    if alert:
        return render(concept.small_alert, pixels)
    return render(concept.small if pixels <= SMALL_MAX else concept.large, pixels)


def zoom(image: Image.Image, factor: int) -> Image.Image:
    return image.resize((image.width * factor, image.height * factor), Image.NEAREST)


def macos(svg_text: str, pixels: int) -> Image.Image:
    return render(build_app_icons.macos_svg(svg_text, 1024), pixels)


class Sheet:
    def __init__(self) -> None:
        self.image = Image.new("RGBA", (WIDTH, 4000), SHEET_BG)
        self.draw = ImageDraw.Draw(self.image)
        self.y = MARGIN

    def text(self, xy, value: str, size: int, colour: str, bold: bool = False) -> None:
        self.draw.text(xy, value, font=font(size, bold), fill=colour)

    def panel(self, height: int, background: str) -> int:
        top = self.y
        self.draw.rounded_rectangle((MARGIN, top, WIDTH - MARGIN, top + height), 24, fill=background)
        self.y += height + 32
        return top

    def section(self, title: str) -> None:
        self.text((MARGIN, self.y), title, 30, "#1d1d1f", bold=True)
        self.y += 52

    def paste(self, image: Image.Image, x: int, y: int) -> None:
        self.image.alpha_composite(image, (x, y))

    def finish(self) -> Image.Image:
        return self.image.crop((0, 0, WIDTH, self.y + MARGIN - 32)).convert("RGB")


def sizes_row(sheet: Sheet, concept: concepts.Concept, background: str, label_colour: str, title: str) -> None:
    sheet.section(title)
    top = sheet.panel(500, background)
    base = top + 40
    x = MARGIN + 40

    big = icon(concept, 1024).resize((384, 384), Image.LANCZOS)
    sheet.paste(big, x, base)
    sheet.text((x, base + 400), "1024 px(缩小到 384 显示)", 20, label_colour)
    x += 384 + 64

    sheet.paste(icon(concept, 256), x, base + 64)
    sheet.text((x, base + 400), "256 px(原大)", 20, label_colour)
    x += 256 + 64

    for pixels, factor in ((32, 8), (16, 16)):
        actual = icon(concept, pixels)
        sheet.paste(actual, x, base + 176)
        sheet.paste(zoom(actual, factor), x + pixels + 24, base + 64)
        sheet.text((x, base + 400), f"{pixels} px 原大 + 放大 {factor} 倍", 20, label_colour)
        x += pixels + 24 + 256 + 64


def browser_tab(concept: concepts.Concept, alert: bool, on_dark: bool, scale: int) -> Image.Image:
    width, height = 220 * scale, 36 * scale
    tab = Image.new("RGBA", (width, height), "#35363a" if on_dark else "#ffffff")
    draw = ImageDraw.Draw(tab)
    favicon = render(concept.small_alert if alert else concept.small, 16 * scale)
    tab.alpha_composite(favicon, (10 * scale, 10 * scale))
    label = "(1) ref · 需要授权" if alert else "ref · my-project"
    draw.text((34 * scale, 8 * scale), label, font=font(13 * scale), fill="#e8eaed" if on_dark else "#202124")
    return tab


def alert_row(sheet: Sheet, concept: concepts.Concept) -> None:
    sheet.section("提醒版本(有会话等你处理时的标签页图标) · Alert variant")
    top = sheet.panel(300, LIGHT_BG)
    base = top + 40
    x = MARGIN + 40
    for pixels, factor in ((32, 6), (16, 12)):
        actual = icon(concept, pixels, alert=True)
        sheet.paste(actual, x, base + 80)
        sheet.paste(zoom(actual, factor), x + pixels + 20, base)
        x += pixels + 20 + 192 + 56
    sheet.text((MARGIN + 40, base + 210), "提醒版 32 px / 16 px,原大 + 放大", 20, LABEL_ON_LIGHT)

    tab_x = x + 40
    for row, on_dark in enumerate((False, True)):
        for column, alert in enumerate((False, True)):
            tab = browser_tab(concept, alert, on_dark, 2)
            sheet.paste(tab, tab_x + column * (tab.width + 24), base + row * (tab.height + 20))
    sheet.text((tab_x, base + 210), "浏览器标签页(2x 屏幕):平时 / 需要处理时,浅色 / 深色", 20, LABEL_ON_LIGHT)


def lockup_row(sheet: Sheet, concept: concepts.Concept) -> None:
    sheet.section("横版组合(README 标题、宣传图) · Lockup")
    top = sheet.panel(360, LIGHT_BG)
    width = 840
    height = round(width * concepts.LOCKUP_HEIGHT / concepts.LOCKUP_WIDTH)
    left = MARGIN + 24
    dark_box = (left + width + 32, top + 24, left + 2 * width + 64, top + 336)
    sheet.draw.rounded_rectangle(dark_box, 16, fill=DARK_BG)
    sheet.paste(render(concept.lockup(on_dark=False), width, height), left + 8, top + 24 + (312 - height) // 2)
    sheet.paste(render(concept.lockup(on_dark=True), width, height), dark_box[0] + 8, top + 24 + (312 - height) // 2)


def dock(concept: concepts.Concept, dark: bool) -> Image.Image:
    """macOS dock at 2x: 64pt icons are 128 device pixels."""
    icon_size, gap, pad = 128, 16, 16
    apps = neighbours.DOCK_LEFT + [concept.large] + neighbours.DOCK_RIGHT
    width = len(apps) * icon_size + (len(apps) - 1) * gap + 2 * pad
    height = icon_size + 2 * pad
    wallpaper = Image.new("RGBA", (width + 80, height + 60), "#3b4a6b" if dark else "#c9d6ea")
    bar = Image.new("RGBA", (width, height), (40, 40, 44, 200) if dark else (255, 255, 255, 150))
    mask = Image.new("L", (width, height), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, width - 1, height - 1), 36, fill=255)
    wallpaper.paste(bar, (40, 30), mask)
    for index, svg in enumerate(apps):
        wallpaper.alpha_composite(macos(svg, icon_size), (40 + pad + index * (icon_size + gap), 30 + pad))
    return wallpaper


def taskbar(concept: concepts.Concept, dark: bool) -> Image.Image:
    """Windows taskbar at 100%: 32px icons on a 48px bar."""
    apps = neighbours.DOCK_LEFT + ["ours"] + neighbours.DOCK_RIGHT
    slot = 48
    bar = Image.new("RGBA", (len(apps) * slot + 32, 48), "#202020" if dark else "#eeeeee")
    for index, svg in enumerate(apps):
        image = icon(concept, 32) if svg == "ours" else render(svg, 32)
        bar.alpha_composite(image, (16 + index * slot + 8, 8))
        if svg == "ours":
            ImageDraw.Draw(bar).rounded_rectangle((16 + index * slot + 16, 44, 16 + index * slot + 32, 46), 1, fill="#7aa2f7")
    return bar


def context_row(sheet: Sheet, concept: concepts.Concept) -> None:
    sheet.section("和其他应用放在一起 · In the dock and taskbar")
    light_dock, dark_dock = dock(concept, False), dock(concept, True)
    top = sheet.panel(2 * light_dock.height + 100, LIGHT_BG)
    left = MARGIN + 24
    sheet.paste(light_dock, left, top + 24)
    sheet.paste(dark_dock, left + 0, top + 24 + light_dock.height + 16)
    label_y = top + 24 + 2 * light_dock.height + 28
    sheet.text((left, label_y), "macOS Dock(2x 屏幕,按 Apple 网格留边),浅色 / 深色", 20, LABEL_ON_LIGHT)

    bars_x = left + light_dock.width + 24
    for row, dark in enumerate((False, True)):
        bar = zoom(taskbar(concept, dark), 2)
        sheet.paste(bar, bars_x, top + 24 + row * (bar.height + 24))
    sheet.text((bars_x, top + 24 + 2 * (96 + 24)), "Windows 任务栏:100% 缩放下的 32 px 图标,", 20, LABEL_ON_LIGHT)
    sheet.text((bars_x, top + 24 + 2 * (96 + 24) + 30), "放大 2 倍以便查看;浅色 / 深色", 20, LABEL_ON_LIGHT)


def build(concept: concepts.Concept) -> Path:
    folder = HERE / concept.key
    folder.mkdir(exist_ok=True)
    sources = {"ref.svg": concept.large, "ref-small.svg": concept.small, "ref-small-alert.svg": concept.small_alert,
               "ref-lockup-dark.svg": concept.lockup(on_dark=True), "ref-lockup-light.svg": concept.lockup(on_dark=False)}
    for name, text in sources.items():
        (folder / name).write_text(text, encoding="utf-8")

    sheet = Sheet()
    sheet.text((MARGIN, sheet.y), concept.title, 52, "#1d1d1f", bold=True)
    sheet.text((MARGIN + 520, sheet.y + 18), concept.title_en, 30, "#6b6d75")
    sheet.y += 84
    sheet.text((MARGIN, sheet.y), concept.idea, 26, "#1d1d1f")
    sheet.y += 44
    sheet.text((MARGIN, sheet.y), concept.idea_en, 22, "#6b6d75")
    sheet.y += 64

    sizes_row(sheet, concept, DARK_BG, LABEL_ON_DARK, "深色背景上 · On dark")
    sizes_row(sheet, concept, LIGHT_BG, LABEL_ON_LIGHT, "浅色背景上 · On light")
    alert_row(sheet, concept)
    lockup_row(sheet, concept)
    context_row(sheet, concept)

    path = folder / f"{concept.key}-sheet.png"
    sheet.finish().save(path, optimize=True)
    return path


def main() -> None:
    for concept in concepts.all_concepts():
        print(build(concept))


if __name__ == "__main__":
    main()
