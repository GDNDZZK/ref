// Where the tiles of the split view go.
//
//   cols = min(ceil(sqrt(n)), floor((W - 2*pad + gap) / (minWidth + gap)))   at least 1
//   rows = ceil(n / cols)
//   tile height = max(minHeight, (H - 2*pad - (rows - 1)*gap) / rows)       the grid scrolls if taller than H
//
// The tiles of a partly filled last row share its width.

import { metrics } from '../theme/generated/theme.ts'

export interface TileSpec {
  minWidth: number
  minHeight: number
  gap: number
  padding: number
}

export const TILE_SPEC: TileSpec = {
  minWidth: metrics.tileMinWidth,
  minHeight: metrics.tileMinHeight,
  gap: metrics.tileGap,
  padding: metrics.tilePadding,
}

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export function tileRects(count: number, width: number, height: number, spec: TileSpec): Rect[] {
  if (count === 0) return []
  const { minWidth, minHeight, gap, padding } = spec
  const fitting = Math.max(1, Math.floor((width - 2 * padding + gap) / (minWidth + gap)))
  const columns = Math.min(Math.ceil(Math.sqrt(count)), fitting)
  const rows = Math.ceil(count / columns)
  const tileHeight = Math.max(minHeight, (height - 2 * padding - (rows - 1) * gap) / rows)

  const rects: Rect[] = []
  for (let row = 0; row < rows; row++) {
    const inRow = Math.min(columns, count - row * columns)
    const tileWidth = (width - 2 * padding - (inRow - 1) * gap) / inRow
    for (let column = 0; column < inRow; column++) {
      rects.push({
        x: padding + column * (tileWidth + gap),
        y: padding + row * (tileHeight + gap),
        width: tileWidth,
        height: tileHeight,
      })
    }
  }
  return rects
}

// Total height of the grid: what the content area scrolls over.
export function gridHeight(rects: readonly Rect[], padding: number): number {
  const bottom = Math.max(0, ...rects.map((rect) => rect.y + rect.height))
  return rects.length === 0 ? 0 : bottom + padding
}

const OFF_AXIS_WEIGHT = 3

// The tile nearest to `from` in the direction (dx, dy), each -1, 0 or 1, by distance between centers.
// Distance off the direction's axis counts more, so moving right prefers the tile in the same row.
export function neighbor(rects: readonly Rect[], from: number, dx: number, dy: number): number | undefined {
  const origin = rects[from]
  if (!origin) return undefined
  const center = (rect: Rect) => ({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 })
  const start = center(origin)

  let best: { index: number; distance: number } | undefined
  rects.forEach((rect, index) => {
    if (index === from) return
    const offset = { x: center(rect).x - start.x, y: center(rect).y - start.y }
    const inDirection = offset.x * dx + offset.y * dy > 0
    if (!inDirection) return
    const along = dx !== 0 ? Math.abs(offset.x) : Math.abs(offset.y)
    const across = dx !== 0 ? Math.abs(offset.y) : Math.abs(offset.x)
    const distance = along + OFF_AXIS_WEIGHT * across
    if (!best || distance < best.distance) best = { index, distance }
  })
  return best?.index
}
