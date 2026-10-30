import { describe, expect, it } from 'vitest'
import { gridHeight, neighbor, tileRects } from './tileLayout.ts'

const spec = { minWidth: 420, minHeight: 300, gap: 12, padding: 16 }

describe('tileRects', () => {
  it('is empty without tiles', () => {
    expect(tileRects(0, 1000, 800, spec)).toEqual([])
  })

  it('fills the area with one tile', () => {
    expect(tileRects(1, 1000, 800, spec)).toEqual([{ x: 16, y: 16, width: 968, height: 768 }])
  })

  it('uses as many columns as ceil(sqrt(n)) when they fit', () => {
    const rects = tileRects(4, 1000, 800, spec)
    expect(rects.map((rect) => [rect.x, rect.y])).toEqual([
      [16, 16],
      [16 + 478 + 12, 16],
      [16, 16 + 378 + 12],
      [16 + 478 + 12, 16 + 378 + 12],
    ])
    expect(rects[0]).toMatchObject({ width: 478, height: 378 })
  })

  it('uses fewer columns when the tiles would get narrower than the minimum', () => {
    const rects = tileRects(4, 700, 2000, spec)
    expect(new Set(rects.map((rect) => rect.x)).size).toBe(1)
  })

  it('lets the tiles of the last row share its width', () => {
    const rects = tileRects(3, 1000, 800, spec)
    expect(rects[2]).toMatchObject({ x: 16, width: 968 })
  })

  it('keeps the minimum height and reports a taller grid', () => {
    const rects = tileRects(6, 700, 400, spec)
    expect(rects[0]!.height).toBe(300)
    expect(gridHeight(rects, 16)).toBeGreaterThan(400)
  })
})

describe('neighbor', () => {
  const rects = tileRects(4, 1000, 800, spec) // 2 x 2

  it('moves to the tile in that direction', () => {
    expect(neighbor(rects, 0, 1, 0)).toBe(1)
    expect(neighbor(rects, 0, 0, 1)).toBe(2)
    expect(neighbor(rects, 3, -1, 0)).toBe(2)
    expect(neighbor(rects, 3, 0, -1)).toBe(1)
  })

  it('stays when there is no tile that way', () => {
    expect(neighbor(rects, 0, -1, 0)).toBeUndefined()
    expect(neighbor(rects, 1, 1, 0)).toBeUndefined()
  })
})
