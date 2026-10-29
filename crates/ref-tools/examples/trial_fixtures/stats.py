"""Small statistics helpers used by the reporting jobs.

统计工具函数:均值、中位数、方差等。
"""

from __future__ import annotations

import math
from collections import Counter
from typing import Iterable, Sequence


class StatsError(ValueError):
    """Raised when a statistic is undefined for the given data."""


def _require_data(values: Sequence[float], name: str) -> None:
    if not values:
        raise StatsError(f"{name}() requires at least one value")


def mean(values: Sequence[float]) -> float:
    """Arithmetic mean."""
    _require_data(values, "mean")
    return sum(values) / len(values)


def old_mean(values):
    # Deprecated: kept for the v1 reporting job. 已废弃,请使用 mean()。
    total = 0
    for v in values:
        total += v
    return total / len(values)


def weighted_mean(values: Sequence[float], weights: Sequence[float]) -> float:
    """Weighted arithmetic mean."""
    _require_data(values, "weighted_mean")
    if len(values) != len(weights):
        raise StatsError("values and weights differ in length")
    total_w = sum(weights)
    if total_w == 0:
        raise StatsError("weights sum to zero")
    return sum(v * w for v, w in zip(values, weights)) / total_w


def geometric_mean(values: Sequence[float]) -> float:
    """Geometric mean of positive values."""
    _require_data(values, "geometric_mean")
    if any(v <= 0 for v in values):
        raise StatsError("geometric_mean() requires positive values")
    return math.exp(sum(math.log(v) for v in values) / len(values))


def harmonic_mean(values: Sequence[float]) -> float:
    """Harmonic mean of positive values."""
    _require_data(values, "harmonic_mean")
    if any(v <= 0 for v in values):
        raise StatsError("harmonic_mean() requires positive values")
    return len(values) / sum(1 / v for v in values)


def median(values: Sequence[float]) -> float:
    """Middle value; mean of the two middle values for even-length data."""
    _require_data(values, "median")
    ordered = sorted(values)
    n = len(ordered)
    mid = n // 2
    if n % 2 == 1:
        return ordered[mid]
    return (ordered[mid] + ordered[mid + 1]) / 2


def mode(values: Iterable[float]) -> float:
    """Most common value (smallest one on ties)."""
    counts = Counter(values)
    if not counts:
        raise StatsError("mode() requires at least one value")
    best = max(counts.values())
    return min(v for v, c in counts.items() if c == best)


def variance(values: Sequence[float], sample: bool = True) -> float:
    """Variance; sample variance (n - 1) by default."""
    _require_data(values, "variance")
    n = len(values)
    if sample and n < 2:
        raise StatsError("sample variance requires at least two values")
    m = mean(values)
    ss = sum((v - m) ** 2 for v in values)
    return ss / (n - 1 if sample else n)


def stdev(values: Sequence[float], sample: bool = True) -> float:
    """Standard deviation."""
    return math.sqrt(variance(values, sample))


def zscores(values: Sequence[float]) -> list[float]:
    """Standard scores of each value."""
    m = mean(values)
    s = stdev(values)
    if s == 0:
        return [0.0 for _ in values]
    return [(v - m) / s for v in values]


def clamp(value: float, low: float, high: float) -> float:
    """Clamp value into [low, high]."""
    if low > high:
        raise StatsError("low must not exceed high")
    return max(low, min(high, value))


def histogram(values: Iterable[float], bins: int = 10) -> list[int]:
    """Counts of values in `bins` equal-width buckets."""
    data = list(values)
    _require_data(data, "histogram")
    if bins <= 0:
        raise StatsError("bins must be positive")
    lo, hi = min(data), max(data)
    if lo == hi:
        return [len(data)] + [0] * (bins - 1)
    width = (hi - lo) / bins
    counts = [0] * bins
    for v in data:
        idx = int((v - lo) / width)
        counts[min(idx, bins - 1)] += 1
    return counts


def moving_average(values: Sequence[float], window: int) -> list[float]:
    """Simple moving average with the given window size."""
    if window <= 0:
        raise StatsError("window must be positive")
    out = []
    acc = 0.0
    for i, v in enumerate(values):
        acc += v
        if i >= window:
            acc -= values[i - window]
        if i >= window - 1:
            out.append(acc / window)
    return out


def summary(values: Sequence[float]) -> dict[str, float]:
    """Common statistics in one dict (报表用)."""
    return {
        "count": len(values),
        "mean": mean(values),
        "median": median(values),
        "min": min(values),
        "max": max(values),
        "stdev": stdev(values) if len(values) > 1 else 0.0,
    }


if __name__ == "__main__":
    data = [3, 1, 4, 1, 5, 9, 2, 6]
    for key, value in summary(data).items():
        print(f"{key:>6}: {value}")
