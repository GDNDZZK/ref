"""Generic stand-ins for other apps in the dock and taskbar mock-ups. No real brands."""

import geometry as g


def _icon(background: str, body: str) -> str:
    return g.svg_doc(1024, 1024, f'  <rect width="1024" height="1024" rx="230" fill="{background}"/>\n{body}')


def _line(d: str, colour: str, width: int) -> str:
    return (f'  <path d="{d}" fill="none" stroke="{colour}" stroke-width="{width}" '
            f'stroke-linecap="round" stroke-linejoin="round"/>')


FILES = _icon("#3b82f6", '  <path d="M200 330a60 60 0 0 1 60-60h170l70 70h264a60 60 0 0 1 60 60v334a60 60 0 0 1-60 60H260a60 60 0 0 1-60-60z" fill="#dbeafe"/>')
BROWSER = _icon("#f4f4f5", '  <circle cx="512" cy="512" r="300" fill="#0ea5e9"/>\n'
                + _line("M212 512h600M512 212a460 460 0 0 1 0 600M512 212a460 460 0 0 0 0 600", "#e0f2fe", 40))
TERMINAL = _icon("#111111", _line("M300 380l140 132-140 132M500 660h220", "#4ade80", 64))
NOTES = _icon("#fde68a", _line("M300 360h424M300 512h424M300 664h260", "#92400e", 56))
SETTINGS = _icon("#9ca3af", '  <circle cx="512" cy="512" r="230" fill="none" stroke="#f3f4f6" stroke-width="90" stroke-dasharray="90 60"/>\n'
                 '  <circle cx="512" cy="512" r="110" fill="#f3f4f6"/>')
MAIL = _icon("#22c55e", '  <rect x="230" y="320" width="564" height="384" rx="50" fill="#f0fdf4"/>\n'
             + _line("M260 360l252 190 252-190", "#22c55e", 44))

DOCK_LEFT = [FILES, BROWSER, TERMINAL]
DOCK_RIGHT = [NOTES, SETTINGS, MAIL]
