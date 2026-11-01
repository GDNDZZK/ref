import { describe, expect, it } from 'vitest'
import { placeInPane } from './popoverPlacement.ts'

const PANE = { left: 500, right: 1000 }

describe('placeInPane', () => {
  it('aligns the right edges when there is room', () => {
    expect(placeInPane({ left: 700, right: 900 }, PANE, 360, 8)).toEqual({ left: 540, width: 360 })
  })

  it('stays inside the left edge of the pane when the button is near it', () => {
    expect(placeInPane({ left: 510, right: 600 }, PANE, 360, 8)).toEqual({ left: 508, width: 360 })
  })

  it('stays inside the right edge of the pane when the button sticks out', () => {
    expect(placeInPane({ left: 950, right: 1100 }, PANE, 360, 8)).toEqual({ left: 632, width: 360 })
  })

  it('narrows to the pane when the pane is narrower than the popover', () => {
    expect(placeInPane({ left: 520, right: 600 }, { left: 500, right: 700 }, 360, 8)).toEqual({
      left: 508,
      width: 184,
    })
  })

  it('has no room at all in a pane narrower than its margins: zero width, at the right margin', () => {
    expect(placeInPane({ left: 505, right: 509 }, { left: 500, right: 510 }, 360, 8)).toEqual({
      left: 502,
      width: 0,
    })
  })
})
