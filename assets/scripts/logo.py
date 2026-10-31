"""Geometry of the program logo. build_app_icons.py writes the SVG sources in assets/app/ from it.

Large mark (1024 units, from 64 px up and on macOS): a dark tile split into four panes that hold
r, e, f and the blue dot, like ref's split view.

    margin = gutter = 80, pane 392, pane radius = tile radius - margin (concentric corners)
    +----------------------+      r, e   x-height band centred in the pane; e's ink box centred across
    |  [ r ]      [ e ]    |      f      ink box centred vertically
    |                      |      r, f   one stem line for the left column. A letter with a stem looks
    |  [ f ]      [ . ]    |             centred on the point halfway between its stem and its ink box
    +----------------------+             centre; the stem line puts that point of the r on the pane
                                         centre, and the f follows (its point 0.63 u left of centre)
                                  dot    pane centre; the r pane and the dot pane sit on the diagonal

Small mark (32 units, 16-48 px and the favicons): the r and the dot, set as "r." with the dot on the
r's baseline, as far from the stem as from the arm. Every straight edge is on an even coordinate, so
16 and 32 px have whole-pixel edges; the ink box is centred on the tile.

The letters are one monoline set in letter units (u): x-height 6, f ascender to -3.5, stroke 1.6
(1/7 of the f's height, as the 20-grid interface icons' stroke 2 on a 14-unit glyph). The r has the
small mark's proportions: shoulder radius 6/12 and arm 2/12 of its stem.
"""

from fractions import Fraction

INK = "#e6e6e9"
INK_ON_LIGHT = "#1d1d1f"
ACCENT = "#7aa2f7"
DANGER = "#ec6f73"
TILE_TOP = "#23242a"
TILE_BOTTOM = "#141518"
PANE = "#2e3038"
RIM_OPACITY = 0.08  # the tile's light edge, so it stands apart from a dark dock or page

# Letters, in u.
X_HEIGHT = 6
STROKE = 1.6
R_SHOULDER = float(Fraction(6, 12) * X_HEIGHT)
R_ARM_END = float(Fraction(8, 12) * X_HEIGHT)
LETTERS = {
    "r": f"M0 {X_HEIGHT}V{R_SHOULDER:.4f}a{R_SHOULDER:.4f} {R_SHOULDER:.4f} 0 0 1 {R_SHOULDER:.4f}-{R_SHOULDER:.4f}H{R_ARM_END:.4f}",
    "e": "M0 3H6A3 3 0 1 0 5.1213 5.1213",
    "f": "M0 6V-0.5a3 3 0 0 1 3-3M-1.5 0H3",
}
HALF = STROKE / 2
INK_BOX = {  # left, top, right, bottom
    "r": (-HALF, -HALF, R_ARM_END + HALF, X_HEIGHT + HALF),
    "e": (-HALF, -HALF, 6 + HALF, X_HEIGHT + HALF),
    "f": (-1.5 - HALF, -3.5 - HALF, 3 + HALF, X_HEIGHT + HALF),
}
STEM_X = 0  # r and f both have their stem at x = 0

# Large mark.
TILE = 1024
TILE_RADIUS = 230
RIM_WIDTH = 12
MARGIN = 80
PANE_SIZE = (TILE - 3 * MARGIN) / 2
PANE_RADIUS = TILE_RADIUS - MARGIN
LETTER_SCALE = 22  # units per u
DOT_RADIUS = 3.4  # u; about the ink of the e

# Small mark.
SMALL_TILE = 32
SMALL_TILE_RADIUS = 7
SMALL_STROKE = 4
SMALL_R = "M10 22V16a6 6 0 0 1 6-6H18"  # stem x 10, top 10, baseline 22, shoulder radius 6, arm 2
# Edges 16..24: its bottom is the r's baseline edge (24), and it keeps 4 units from the stem (edge 12)
# and from the arm (edge 12). The ink box is 8..24 on both axes.
SMALL_DOT = (20, 20)
SMALL_DOT_RADIUS = 4  # the alert version only changes the dot's colour

# Lockup: the large mark and the word "ref".
LOCKUP_HEIGHT = 360
LOCKUP_MARK = 280
LOCKUP_PAD = 40
LOCKUP_GAP = 56
LOCKUP_SCALE = 20
WORD_OFFSETS = {"r": 0.0, "e": 6.4, "f": 16.3}  # 0.8 u of air between the letters


def svg_doc(width: float, height: float, body: str, defs: str = "") -> str:
    defs_block = f"\n  <defs>{defs}\n  </defs>" if defs else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:g}" height="{height:g}" '
            f'viewBox="0 0 {width:g} {height:g}">{defs_block}\n{body}\n</svg>\n')


def letter(ch: str, x: float, y: float, scale: float, colour: str) -> str:
    """One letter with its origin (stem foot's x, x-height line) at (x, y)."""
    return (f'  <path transform="translate({x:.2f} {y:.2f}) scale({scale:g})" d="{LETTERS[ch]}" fill="none" '
            f'stroke="{colour}" stroke-width="{STROKE:g}" stroke-linecap="round" stroke-linejoin="round"/>')


def pane_origin(index: int) -> tuple[float, float]:
    step = PANE_SIZE + MARGIN
    return MARGIN + (index % 2) * step, MARGIN + (index // 2) * step


def stemmed_optical_x(ch: str) -> float:
    left, _, right, _ = INK_BOX[ch]
    return (STEM_X + (left + right) / 2) / 2


STEMMED = ("r", "f")
STEM_COLUMN_X = stemmed_optical_x("r")  # the r leads; averaging with the f left the r looking off to the right


def letter_in_pane(ch: str, index: int) -> str:
    x, y = pane_origin(index)
    centre_x, centre_y = x + PANE_SIZE / 2, y + PANE_SIZE / 2
    left, top, right, bottom = INK_BOX[ch]
    optical_x = STEM_COLUMN_X if ch in STEMMED else (left + right) / 2
    optical_y = (top + bottom) / 2 if ch == "f" else X_HEIGHT / 2
    return letter(ch, centre_x - optical_x * LETTER_SCALE, centre_y - optical_y * LETTER_SCALE, LETTER_SCALE, INK)


def large_body() -> tuple[str, str]:
    """(defs, shapes) of the large mark."""
    defs = (f'\n    <linearGradient id="ref-tile" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{TILE_TOP}"/><stop offset="1" stop-color="{TILE_BOTTOM}"/></linearGradient>')
    inset = RIM_WIDTH / 2
    parts = [
        f'  <rect width="{TILE}" height="{TILE}" rx="{TILE_RADIUS}" fill="url(#ref-tile)"/>',
        f'  <rect x="{inset:g}" y="{inset:g}" width="{TILE - RIM_WIDTH}" height="{TILE - RIM_WIDTH}" rx="{TILE_RADIUS - inset:g}" '
        f'fill="none" stroke="#ffffff" stroke-opacity="{RIM_OPACITY}" stroke-width="{RIM_WIDTH}"/>',
    ]
    for index in range(4):
        x, y = pane_origin(index)
        parts.append(f'  <rect x="{x:g}" y="{y:g}" width="{PANE_SIZE:g}" height="{PANE_SIZE:g}" rx="{PANE_RADIUS:g}" fill="{PANE}"/>')
    parts += [letter_in_pane(ch, index) for index, ch in enumerate("ref")]
    x, y = pane_origin(3)
    parts.append(f'  <circle cx="{x + PANE_SIZE / 2:g}" cy="{y + PANE_SIZE / 2:g}" r="{DOT_RADIUS * LETTER_SCALE:g}" fill="{ACCENT}"/>')
    return defs, "\n".join(parts)


def large() -> str:
    defs, body = large_body()
    return svg_doc(TILE, TILE, body, defs)


def small(alert: bool = False) -> str:
    colour = DANGER if alert else ACCENT
    return svg_doc(SMALL_TILE, SMALL_TILE, "\n".join([
        f'  <rect width="{SMALL_TILE}" height="{SMALL_TILE}" rx="{SMALL_TILE_RADIUS}" fill="{TILE_BOTTOM}"/>',
        f'  <path d="{SMALL_R}" fill="none" stroke="{INK}" stroke-width="{SMALL_STROKE}" stroke-linecap="round" stroke-linejoin="round"/>',
        f'  <circle cx="{SMALL_DOT[0]}" cy="{SMALL_DOT[1]}" r="{SMALL_DOT_RADIUS}" fill="{colour}"/>',
    ]))


def lockup(on_dark: bool) -> str:
    """The large mark followed by "ref", for the README header and title cards."""
    defs, body = large_body()
    top = (LOCKUP_HEIGHT - LOCKUP_MARK) / 2
    mark = f'  <g transform="translate({LOCKUP_PAD} {top:g}) scale({LOCKUP_MARK / TILE:g})">\n{body}\n  </g>'

    k = LOCKUP_SCALE
    word_left = INK_BOX["r"][0]
    word_right = WORD_OFFSETS["f"] + INK_BOX["f"][2]
    word_top, word_bottom = INK_BOX["f"][1], INK_BOX["f"][3]
    x0 = LOCKUP_PAD + LOCKUP_MARK + LOCKUP_GAP - word_left * k
    y0 = LOCKUP_HEIGHT / 2 - (word_top + word_bottom) / 2 * k
    colour = INK if on_dark else INK_ON_LIGHT
    word = "\n".join(letter(ch, x0 + WORD_OFFSETS[ch] * k, y0, k, colour) for ch in "ref")
    width = x0 + word_right * k + LOCKUP_PAD
    return svg_doc(round(width), LOCKUP_HEIGHT, mark + "\n" + word, defs)
