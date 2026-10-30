#!/usr/bin/env python3
"""Lint assets/icons/*.svg against the grid rules in assets/icons/INDEX.md.

Two sets exist, told apart by the file name suffix:

  -20  20x20 grid, stroke 2, for buttons and toolbars.
       Stroke centre lines sit on integers, so the 2px stroke edges land on
       whole device pixels at 1x and 2x.
  -16  16x16 grid, stroke 1.5, for icons set next to 12-13px text.
       Stroke centre lines sit on .25 / .75, so the 3px stroke edges land on
       whole device pixels at 2x.

Usage: check_icons.py [file.svg ...]   (default: every icon). Exit code 1 on any violation.
"""

import math
import re
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path

ICON_DIR = Path(__file__).resolve().parent.parent / "icons"
INDEX_FILE = ICON_DIR / "INDEX.md"

SVG_NS = "http://www.w3.org/2000/svg"
ALLOWED_TAGS = {"path", "circle", "rect"}
NAME_PATTERN = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*-(16|20)$")
INDEX_ROW_PATTERN = re.compile(r"^\| `([a-z0-9]+(?:-[a-z0-9]+)*)` \| ([\d, ]+) \|")
TOKEN_PATTERN = re.compile(r"[MmLlHhVvAaZz]|-?(?:\d+\.?\d*|\.\d+)")
EPSILON = 1e-6
# A curve's extreme that does not end a segment (e.g. the bulge of a globe's meridian) may be this far
# off the pixel grid: the stroke is anti-aliased there anyway. Straight edges and arc ends stay exact.
CURVE_EXTREME_TOLERANCE = 0.2


@dataclass(frozen=True)
class GridSpec:
    size: int
    stroke_width: float
    pitch: float  # every coordinate is a multiple of this
    centre_fractions: tuple[float, ...]  # fractional parts allowed for axis-aligned stroke centre lines
    margin: float  # centre lines stay within [margin, size - margin]


GRIDS = {
    20: GridSpec(size=20, stroke_width=2, pitch=1, centre_fractions=(0.0,), margin=3),
    16: GridSpec(size=16, stroke_width=1.5, pitch=0.25, centre_fractions=(0.25, 0.75), margin=2.25),
}

ROOT_ATTRIBUTES = {"fill": "none", "stroke": "currentColor", "stroke-linecap": "round", "stroke-linejoin": "round"}
CARDINAL_ANGLES = (0.0, 90.0, 180.0, 270.0)  # east, south, west, north (y points down)


class Linter:
    def __init__(self, path: Path):
        self.path = path
        self.errors: list[str] = []
        self.spec = GRIDS[int(path.stem[-2:])]

    def error(self, message: str) -> None:
        self.errors.append(f"{self.path.name}: {message}")

    # Checks on single numbers.

    def is_on_pitch(self, value: float) -> bool:
        steps = value / self.spec.pitch
        return abs(steps - round(steps)) < EPSILON

    def is_centre_aligned(self, value: float, tolerance: float = EPSILON) -> bool:
        fraction = value % 1
        return any(abs(fraction - f) < tolerance or abs(fraction - f - 1) < tolerance for f in self.spec.centre_fractions)

    def check_lattice(self, what: str, value: float) -> None:
        if not self.is_on_pitch(value):
            self.error(f"{what} = {value:g} is not a multiple of {self.spec.pitch:g}")

    def check_centre(self, what: str, value: float, tolerance: float = EPSILON) -> None:
        if not self.is_centre_aligned(value, tolerance):
            self.error(f"{what} = {value:g} is not a pixel-aligned stroke centre")

    def check_within_margin(self, what: str, low: float, high: float) -> None:
        margin = self.spec.margin
        if low < margin - EPSILON or high > self.spec.size - margin + EPSILON:
            self.error(f"{what} spans {low:g}..{high:g}, outside {margin:g}..{self.spec.size - margin:g}")

    # Shapes.

    def check_horizontal(self, y: float) -> None:
        self.check_centre("horizontal line y", y)

    def check_vertical(self, x: float) -> None:
        self.check_centre("vertical line x", x)

    def check_circle(self, cx: float, cy: float, r: float) -> None:
        for name, value in (("cx", cx), ("cy", cy), ("r", r)):
            self.check_lattice(name, value)
        self.check_centre("circle left tangent", cx - r)
        self.check_centre("circle right tangent", cx + r)
        self.check_centre("circle top tangent", cy - r)
        self.check_centre("circle bottom tangent", cy + r)
        self.check_within_margin("circle x", cx - r, cx + r)
        self.check_within_margin("circle y", cy - r, cy + r)

    def check_arc(self, start, end, radius_x, radius_y, rotation, large, sweep) -> None:
        if radius_x != radius_y or rotation != 0:
            self.error("arcs must be circular (rx == ry, rotation 0)")
            return

        centre = arc_centre(start, end, radius_x, bool(large), bool(sweep))
        if centre is None:
            self.error(f"arc from {start} to {end} cannot have radius {radius_x:g}")
            return

        cx, cy = centre
        start_angle = math.degrees(math.atan2(start[1] - cy, start[0] - cx)) % 360
        end_angle = math.degrees(math.atan2(end[1] - cy, end[0] - cx)) % 360
        low_x, high_x, low_y, high_y = [min(start[0], end[0]), max(start[0], end[0]), min(start[1], end[1]), max(start[1], end[1])]
        for angle in CARDINAL_ANGLES:
            if not angle_on_arc(angle, start_angle, end_angle, bool(sweep)):
                continue
            is_horizontal_extreme = angle in (90.0, 270.0)  # south / north: tangent is horizontal
            is_end = min(abs(angle - start_angle), abs(angle - end_angle)) < EPSILON
            tolerance = EPSILON if is_end else CURVE_EXTREME_TOLERANCE
            rad = math.radians(angle)
            px, py = cx + radius_x * math.cos(rad), cy + radius_x * math.sin(rad)
            if is_horizontal_extreme:
                self.check_centre("arc top/bottom y", py, tolerance)
            else:
                self.check_centre("arc left/right x", px, tolerance)
            low_x, high_x = min(low_x, px), max(high_x, px)
            low_y, high_y = min(low_y, py), max(high_y, py)
        self.check_within_margin("arc x", low_x, high_x)
        self.check_within_margin("arc y", low_y, high_y)

    def check_segment(self, start, end) -> None:
        for point in (start, end):
            self.check_lattice("x", point[0])
            self.check_lattice("y", point[1])
            self.check_within_margin("x", point[0], point[0])
            self.check_within_margin("y", point[1], point[1])

        is_dot = start == end
        if is_dot:
            self.check_horizontal(start[1])
            self.check_vertical(start[0])
        elif start[1] == end[1]:
            self.check_horizontal(start[1])
        elif start[0] == end[0]:
            self.check_vertical(start[0])
        elif abs(abs(end[0] - start[0]) - abs(end[1] - start[1])) > EPSILON:
            slope = (end[1] - start[1]) / (end[0] - start[0])
            if abs(slope) not in (2.0, 0.5):
                self.error(f"segment {start} -> {end} is neither axis-aligned, 45 degrees nor 2:1")

    # Elements.

    def check_rect(self, element: ET.Element) -> None:
        x, y, width, height = (float(element.get(k, "0")) for k in ("x", "y", "width", "height"))
        radius = float(element.get("rx", "0"))
        for name, value in (("x", x), ("y", y), ("width", width), ("height", height), ("rx", radius)):
            self.check_lattice(name, value)
        self.check_centre("rect left", x)
        self.check_centre("rect right", x + width)
        self.check_centre("rect top", y)
        self.check_centre("rect bottom", y + height)
        self.check_within_margin("rect x", x, x + width)
        self.check_within_margin("rect y", y, y + height)

    def check_path(self, data: str) -> None:
        tokens = TOKEN_PATTERN.findall(data)
        index = 0
        command = ""
        current = (0.0, 0.0)
        subpath_start = (0.0, 0.0)

        def take(count: int) -> list[float]:
            nonlocal index
            values = [float(t) for t in tokens[index:index + count]]
            index += count
            return values

        while index < len(tokens):
            if tokens[index].isalpha():
                command = tokens[index]
                index += 1
                if command in "Zz":
                    self.check_segment(current, subpath_start)
                    current = subpath_start
                    continue
            elif command in ("M", "m"):
                command = "L" if command == "M" else "l"  # extra pairs after M are implicit lines

            relative = command.islower()
            base = current if relative else (0.0, 0.0)
            kind = command.upper()

            if kind == "M":
                x, y = take(2)
                current = (base[0] + x, base[1] + y)
                subpath_start = current
            elif kind == "L":
                x, y = take(2)
                target = (base[0] + x, base[1] + y)
                self.check_segment(current, target)
                current = target
            elif kind == "H":
                (x,) = take(1)
                target = (base[0] + x, current[1])
                self.check_segment(current, target)
                current = target
            elif kind == "V":
                (y,) = take(1)
                target = (current[0], base[1] + y)
                self.check_segment(current, target)
                current = target
            elif kind == "A":
                rx, ry, rotation, large, sweep, x, y = take(7)
                target = (base[0] + x, base[1] + y)
                self.check_lattice("arc rx", rx)
                self.check_lattice("arc end x", target[0])
                self.check_lattice("arc end y", target[1])
                self.check_arc(current, target, rx, ry, rotation, large, sweep)
                current = target
            else:
                self.error(f"unsupported path command {command!r}")
                return

    def check_root(self, root: ET.Element) -> None:
        size = str(self.spec.size)
        expected = {"width": size, "height": size, "viewBox": f"0 0 {size} {size}", "stroke-width": f"{self.spec.stroke_width:g}", **ROOT_ATTRIBUTES}
        for name, value in expected.items():
            if root.get(name) != value:
                self.error(f"<svg {name}> must be {value!r}, found {root.get(name)!r}")

    def run(self) -> list[str]:
        if not NAME_PATTERN.match(self.path.stem):
            self.error("name must be kebab-case ending in -16 or -20")
            return self.errors

        root = ET.parse(self.path).getroot()
        if root.tag != f"{{{SVG_NS}}}svg":
            self.error("root element is not <svg>")
            return self.errors

        self.check_root(root)
        shapes = list(root)
        if not shapes:
            self.error("no shapes")

        for element in shapes:
            tag = element.tag.removeprefix(f"{{{SVG_NS}}}")
            extra = set(element.attrib) - {"d", "cx", "cy", "r", "x", "y", "width", "height", "rx"}
            if tag not in ALLOWED_TAGS or extra:
                self.error(f"<{tag}> with attributes {sorted(extra)} is not allowed; colours and strokes come from <svg>")
            elif tag == "path":
                self.check_path(element.get("d", ""))
            elif tag == "circle":
                self.check_circle(*(float(element.get(k, "0")) for k in ("cx", "cy", "r")))
            else:
                self.check_rect(element)
        return self.errors


def arc_centre(start, end, radius, large: bool, sweep: bool):
    """Centre of the circle through start and end, picked by the SVG large-arc and sweep flags."""
    mid_x, mid_y = (start[0] + end[0]) / 2, (start[1] + end[1]) / 2
    half_chord = math.hypot(end[0] - start[0], end[1] - start[1]) / 2
    if half_chord > radius + EPSILON or half_chord == 0:
        return None

    offset = math.sqrt(max(radius * radius - half_chord * half_chord, 0.0))
    unit_x, unit_y = (end[0] - start[0]) / (2 * half_chord), (end[1] - start[1]) / (2 * half_chord)
    normal_x, normal_y = -unit_y, unit_x
    sign = 1 if large != sweep else -1
    return mid_x + sign * offset * normal_x, mid_y + sign * offset * normal_y


def angle_on_arc(angle: float, start: float, end: float, sweep: bool) -> bool:
    """True when `angle` lies on the arc from start to end (degrees; sweep = clockwise on screen)."""
    span = (end - start) % 360 if sweep else (start - end) % 360
    offset = (angle - start) % 360 if sweep else (start - angle) % 360
    return offset <= span + EPSILON


def check_index(files: list[Path]) -> list[str]:
    """Every icon is listed in INDEX.md, and every listed name and set has a file."""
    if not INDEX_FILE.exists():
        return [f"{INDEX_FILE.name}: missing"]

    on_disk = {p.stem for p in files}
    listed_names = set()
    errors = []
    for line in INDEX_FILE.read_text(encoding="utf-8").splitlines():
        row = INDEX_ROW_PATTERN.match(line)
        if not row:
            continue
        name, sets = row.group(1), [int(n) for n in re.findall(r"\d+", row.group(2))]
        listed_names.add(name)
        errors += [f"INDEX.md: {name} lists set {n}, but {name}-{n}.svg does not exist" for n in sets if f"{name}-{n}" not in on_disk]

    for stem in sorted(on_disk):
        if stem[:-3] not in listed_names:
            errors.append(f"{stem}.svg: not listed in INDEX.md")
    errors += [f"INDEX.md: {name} has no file" for name in sorted(listed_names) if not {f"{name}-16", f"{name}-20"} & on_disk]
    return errors


def main() -> int:
    arguments = [Path(a) for a in sys.argv[1:]]
    files = arguments or sorted(ICON_DIR.glob("*.svg"))
    errors = [e for f in files for e in Linter(f).run()]
    if not arguments:
        errors += check_index(files)

    for message in errors:
        print(message)
    print(f"{len(files)} icons, {len(errors)} problems")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
