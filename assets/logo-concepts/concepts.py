"""The three logo concepts, built as SVG strings.

Each concept has a large master (1024 units, full-bleed tile, used from 64px up), a small master
(32 units, used up to 48px and for the favicon; even coordinates so 16 and 32px land on whole pixels),
the small master's alert version, and a horizontal lockup (mark + "ref") for dark and light backgrounds.
"""

from dataclasses import dataclass

import geometry as g

WORD_STROKE = 1.6  # monoline stroke, in letter units
PANE = "#2b2d34"  # an idle window or pane on the tile
PANE_BACK = "#3a3d46"

LOCKUP_WIDTH, LOCKUP_HEIGHT = 920, 360
LOCKUP_MARK = 280
LOCKUP_PAD = 40
LOCKUP_GAP = 56
LOCKUP_WORD_K = 20.0


@dataclass(frozen=True)
class Concept:
    key: str
    title: str
    title_en: str
    idea: str
    idea_en: str
    large: str
    small: str
    small_alert: str
    lockup_dot: bool  # the wordmark in the lockup ends with the caret dot
    lockup_uses_small: bool  # the lockup's mark is the small master (when the large one already spells ref)

    def lockup(self, on_dark: bool) -> str:
        ink = g.INK if on_dark else g.INK_ON_LIGHT
        top = (LOCKUP_HEIGHT - LOCKUP_MARK) / 2
        source, units = (self.small, 32) if self.lockup_uses_small else (self.large, 1024)
        mark = f'  <g transform="translate({LOCKUP_PAD} {top:g}) scale({LOCKUP_MARK / units:.6f})">\n{body_of(source)}\n  </g>'

        k = LOCKUP_WORD_K
        left, word_top, _, bottom = g.word_box(WORD_STROKE, self.lockup_dot)
        x0 = LOCKUP_PAD + LOCKUP_MARK + LOCKUP_GAP - left * k
        y0 = LOCKUP_HEIGHT / 2 - (word_top + bottom) / 2 * k
        parts = [mark, g.word(x0, y0, k, WORD_STROKE, ink)]
        if self.lockup_dot:
            parts.append(dot(x0 + g.DOT_X * k, y0 + g.dot_y(WORD_STROKE) * k, g.DOT_R * k, g.ACCENT))
        return g.svg_doc(LOCKUP_WIDTH, LOCKUP_HEIGHT, "\n".join(parts))


def body_of(svg_text: str) -> str:
    """Everything inside <svg>, defs included (ids are shared, so one mark per document)."""
    start = svg_text.index(">", svg_text.index("<svg")) + 1
    return svg_text[start:svg_text.rindex("</svg>")].strip("\n")


def large(body: str) -> str:
    return g.svg_doc(1024, 1024, g.large_tile() + "\n" + body, g.tile_gradient())


def small(body: str) -> str:
    return g.svg_doc(32, 32, g.small_tile() + "\n" + body)


def stroke(d: str, colour: str, width: float) -> str:
    return (f'  <path d="{d}" fill="none" stroke="{colour}" stroke-width="{width:g}" '
            f'stroke-linecap="round" stroke-linejoin="round"/>')


def dot(x: float, y: float, r: float, colour: str) -> str:
    return f'  <circle cx="{x:.2f}" cy="{y:.2f}" r="{r:.2f}" fill="{colour}"/>'


def rect(x: float, y: float, w: float, h: float, r: float, colour: str) -> str:
    return f'  <rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}" rx="{r:g}" fill="{colour}"/>'


def centred_word(cx: float, cy: float, k: float, colour: str, with_dot: bool) -> str:
    left, top, right, bottom = g.word_box(WORD_STROKE, with_dot)
    x0 = cx - (left + right) / 2 * k
    y0 = cy - (top + bottom) / 2 * k
    parts = [g.word(x0, y0, k, WORD_STROKE, colour)]
    if with_dot:
        parts.append(dot(x0 + g.DOT_X * k, y0 + g.dot_y(WORD_STROKE) * k, g.DOT_R * k, g.ACCENT))
    return "\n".join(parts)


# A: the word itself, in the interface's monoline stroke, ending in the streaming caret.

def concept_wordmark() -> Concept:
    left, _, right, _ = g.word_box(WORD_STROKE, True)
    k = 744 / (right - left)

    def monogram(marker: str, radius: float) -> str:
        # r: stem x 6 (edges 4..8), shoulder radius 10 so its counter stays open at 16px; caret dot 20..28
        return small(stroke("M6 8V24M6 18a10 10 0 0 1 10-10h2", g.INK, 4) + "\n" + dot(24, 22, radius, marker))

    return Concept(
        "a-wordmark", "方案 A · 文字标", "Concept A · Wordmark",
        "直接写出 ref 三个字母,笔画和界面图标同款(圆头单线);末尾的蓝点是回复流式输出时的光标。小尺寸缩成 r 加蓝点。",
        "The word ref in the interface's round monoline stroke, ending in the blue streaming caret. Small sizes: r and the dot.",
        large(centred_word(512, 512, k, g.INK, True)),
        monogram(g.ACCENT, 4), monogram(g.DANGER, 5), False, True)


# B: the split view. Three panes spell r e f, the fourth is the session at work.

def concept_panes() -> Concept:
    margin, gap, radius = 104, 40, 72
    size = (1024 - 2 * margin - gap) / 2
    k = 30.0
    parts = []
    for index, ch in enumerate("ref"):
        px = margin + (index % 2) * (size + gap)
        py = margin + (index // 2) * (size + gap)
        parts.append(rect(px, py, size, size, radius, PANE))
        cx, cy = g.LETTER_CENTRE[ch]
        parts.append(g.letter(ch, px + size / 2 - cx * k, py + size / 2 - cy * k, k, WORD_STROKE, g.INK))
    active = margin + size + gap
    parts.append(rect(active, active, size, size, radius, g.ACCENT))
    parts.append(dot(active + size / 2, active + size / 2, 44, g.ON_ACCENT))

    def tiles(marker: str) -> str:
        # a tall pane with r (stem x 8, caps 8..26) and two side panes; every edge even
        return small(rect(2, 2, 20, 28, 4, PANE) + "\n"
                     + stroke("M8 10V24M8 18a8 8 0 0 1 8-8", g.INK, 4) + "\n"
                     + rect(24, 2, 6, 12, 3, marker) + "\n"
                     + rect(24, 18, 6, 12, 3, PANE))

    return Concept(
        "b-panes", "方案 B · 分屏四格", "Concept B · Split panes",
        "取自 ref 的分屏视图:四个窗格里依次是 r、e、f,第四格是蓝色的、正在工作的会话。提示\"一个后端,多个会话并排工作\"。",
        "ref's split view: three panes spell r e f, the fourth (blue) is a session at work. Many sessions, side by side.",
        large("\n".join(parts)), tiles(g.ACCENT), tiles(g.DANGER), False, False)


# C: a stack of windows. Several windows on one backend; the front one, in accent blue, says ref.

def concept_windows() -> Concept:
    width, height, radius, step = 680, 560, 80, 56
    front_x, front_y = 116, 288
    parts = [
        rect(front_x + 2 * step, front_y - 2 * step, width, height, radius, "#2e3038"),
        rect(front_x + step, front_y - step, width, height, radius, "#454852"),
        rect(front_x, front_y, width, height, radius, g.ACCENT),
        centred_word(front_x + width / 2, front_y + height / 2, 27.0, g.ON_ACCENT, False),
    ]

    def stack(back: str) -> str:
        # back window 10..30 x 2..22, front 2..24 x 8..30; r: stem x 10 (edges 8..12), caps 12..26
        return small(rect(10, 2, 20, 20, 4, back) + "\n"
                     + rect(2, 8, 22, 22, 4, g.ACCENT) + "\n"
                     + stroke("M10 14V24M10 20a6 6 0 0 1 6-6h1", g.ON_ACCENT, 4))

    return Concept(
        "c-windows", "方案 C · 叠放的窗口", "Concept C · Stacked windows",
        "几个叠在一起的窗口共用一个后端,最前面的蓝色窗口写着 ref。提醒时后面的窗口变红:另一个会话在等你。",
        "Several windows on one backend; the front, blue one says ref. For alerts the window behind turns red: another session needs you.",
        large("\n".join(parts)), stack("#454852"), stack(g.DANGER), False, False)


def all_concepts() -> list[Concept]:
    return [concept_wordmark(), concept_panes(), concept_windows()]
