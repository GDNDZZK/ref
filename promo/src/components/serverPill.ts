import { span } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'

export interface ServerPillOptions {
  // Seconds on the timer when the pill appears.
  timerStart: number
  from: number
  to: number
}

const ENTER_SECONDS = 0.5
const EXIT_SECONDS = 0.3
const SECONDS_PER_MINUTE = 60

function clock(totalSeconds: number): string {
  const whole = Math.floor(totalSeconds)
  const minutes = Math.floor(whole / SECONDS_PER_MINUTE)
  const seconds = whole % SECONDS_PER_MINUTE
  return `${minutes}:${String(seconds).padStart(2, '0')}`
}

// "ref-server · session working 0:42": the backend keeps running while windows come and go.
export function serverPill(options: ServerPillOptions): Part {
  const el = element('div', 'part server-pill')
  element('span', 'server-pill-dot', el)
  element('span', 'server-pill-name', el).textContent = 'ref-server'
  const timer = element('span', 'server-pill-timer', el)

  return {
    el,
    render(time) {
      timer.textContent = clock(options.timerStart + Math.max(0, time - options.from))
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
    },
  }
}
