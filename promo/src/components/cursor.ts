import { span, track, type Key } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'

export interface Point {
  x: number
  y: number
}

export interface PathKey {
  at: number
  x: number
  y: number
  ease?: Key['ease']
}

const RIPPLE_SECONDS = 0.5
const RIPPLE_START_SCALE = 0.2
const PRESS_SECONDS = 0.16
const PRESS_SCALE = 0.85

// A position over time through waypoints, in stage pixels.
export function pointerPath(keys: readonly PathKey[]): (time: number) => Point {
  const x = track(keys.map(({ at, x, ease }) => ({ at, value: x, ease })))
  const y = track(keys.map(({ at, y, ease }) => ({ at, value: y, ease })))
  return (time) => ({ x: x(time), y: y(time) })
}

const ARROW_SVG =
  '<svg width="34" height="34" viewBox="0 0 24 24"><path d="M5 3l14 8-6 1.5L10 19z" ' +
  'fill="#fff" stroke="#16171a" stroke-width="1.6" stroke-linejoin="round"/></svg>'

// Mouse pointer that follows `position` and ripples at each click time.
export function cursor(position: (time: number) => Point, clicks: readonly number[] = []): Part {
  const el = element('div', 'part cursor')
  el.innerHTML = ARROW_SVG
  const ripple = element('div', 'cursor-ripple', el)

  return {
    el,
    render(time) {
      const { x, y } = position(time)
      const pressed = clicks.some((click) => time >= click && time < click + PRESS_SECONDS)
      el.style.transform = `translate(${x}px, ${y}px) scale(${pressed ? PRESS_SCALE : 1})`

      const lastClick = clicks.filter((click) => click <= time).at(-1)
      const progress = lastClick === undefined ? 1 : span(time, lastClick, RIPPLE_SECONDS, 'outExpo')
      ripple.style.opacity = String(progress >= 1 ? 0 : 1 - progress)
      ripple.style.transform = `translate(-50%, -50%) scale(${RIPPLE_START_SCALE + (1 - RIPPLE_START_SCALE) * progress})`
    },
  }
}
