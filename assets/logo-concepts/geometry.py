"""Shared pieces for the logo concepts: palette, monoline letters r/e/f, SVG document helpers.

Letters are monoline paths in letter units (u) with round caps, the same stroke language as the
interface icons. x-height is 6u, f rises to -3.5u, the baseline is y = 6.
"""

INK = "#e6e6e9"
INK_ON_LIGHT = "#1d1d1f"
ACCENT = "#7aa2f7"
DANGER = "#ec6f73"
TILE = "#16171a"
TILE_TOP = "#25262b"
ON_ACCENT = "#10131a"

LETTERS = {
    "r": "M0 0V6M0 3a3 3 0 0 1 3-3",
    "e": "M0 3H6A3 3 0 1 0 5.121 5.121",
    "f": "M0 6V-0.5a3 3 0 0 1 3-3M-1.5 0H3",
}
# bounding box centre of each letter, in u (x, y)
LETTER_CENTRE = {"r": (1.5, 3.0), "e": (3.0, 3.0), "f": (0.75, 1.25)}

# Offsets of r, e, f when set as a word, chosen so strokes of 1.6u keep 0.8u between letters.
WORD_OFFSETS = {"r": 0.0, "e": 5.4, "f": 15.3}
WORD_RIGHT = 18.3  # right end of the f hook
WORD_ASCENDER = -3.5
WORD_BASELINE = 6.0


def svg_doc(width: float, height: float, body: str, defs: str = "") -> str:
    defs_block = f"\n  <defs>{defs}</defs>" if defs else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:g}" height="{height:g}" '
            f'viewBox="0 0 {width:g} {height:g}">{defs_block}\n{body}\n</svg>\n')


def tile_gradient() -> str:
    return (f'\n    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{TILE_TOP}"/><stop offset="1" stop-color="{TILE}"/></linearGradient>\n  ')


def large_tile() -> str:
    """1024-unit full-bleed tile, as in assets/app/ref.svg."""
    return (f'  <rect width="1024" height="1024" rx="230" fill="url(#tile)"/>\n'
            f'  <rect x="6" y="6" width="1012" height="1012" rx="224" fill="none" stroke="#ffffff" '
            f'stroke-opacity="0.08" stroke-width="12"/>')


def small_tile() -> str:
    return f'  <rect width="32" height="32" rx="7" fill="{TILE}"/>'


def letter(ch: str, x: float, y: float, k: float, stroke: float, colour: str) -> str:
    """One letter with its origin at (x, y) in the target space, scaled by k."""
    return (f'<path transform="translate({x:.3f} {y:.3f}) scale({k:.4f})" d="{LETTERS[ch]}" fill="none" '
            f'stroke="{colour}" stroke-width="{stroke:g}" stroke-linecap="round" stroke-linejoin="round"/>')


def word(x: float, y: float, k: float, stroke: float, colour: str) -> str:
    """'ref' with the r stem at x and the baseline at y + 6k."""
    return "\n".join(letter(ch, x + WORD_OFFSETS[ch] * k, y, k, stroke, colour) for ch in "ref")


def word_width(stroke: float) -> float:
    return WORD_RIGHT + stroke  # u, including half a stroke on each side


def word_height(stroke: float) -> float:
    return WORD_BASELINE - WORD_ASCENDER + stroke

# The caret dot that ends the word: under the end of the f's arm, sitting on the baseline.
DOT_X = 18.3
DOT_R = 1.0


def dot_y(stroke: float) -> float:
    return WORD_BASELINE + stroke / 2 - DOT_R


def word_box(stroke: float, with_dot: bool) -> tuple[float, float, float, float]:
    """(left, top, right, bottom) of the word's ink, in u."""
    right = max(WORD_RIGHT + stroke / 2, DOT_X + DOT_R if with_dot else 0)
    return -stroke / 2, WORD_ASCENDER - stroke / 2, right, WORD_BASELINE + stroke / 2
