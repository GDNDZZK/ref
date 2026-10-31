import { span } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'
import type { Rect } from './appFrame.ts'
import { createWindow } from './windowChrome.ts'

export interface WindowOutlineOptions {
  rect: Rect
  from: number
  to: number
}

const ENTER_SECONDS = 0.5
const EXIT_SECONDS = 0.3

// An empty browser window, drawn as an outline: the place where the run is not.
export function windowOutline(options: WindowOutlineOptions): Part {
  const { frame } = createWindow('')
  frame.classList.add('window-outline')
  const { rect } = options
  Object.assign(frame.style, { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.width}px`, height: `${rect.height}px` })
  element('div', 'window-outline-address', frame.querySelector('.window-bar')!)

  return {
    el: frame,
    render(time) {
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      frame.style.opacity = String(entering * (1 - leaving))
    },
  }
}
