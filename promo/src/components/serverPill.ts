import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface ServerPillOptions {
  // Seconds on the timer at `from`; it counts in real time from there.
  timerStart: number
  // Cuts that skip time: from `at` on the timer is `seconds` further.
  jumps?: readonly { at: number; seconds: number }[]
  // Text after the timer from `from` on, e.g. "5 sessions running" (from the real take).
  sessions?: { text: string | Localized; from: number }
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
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
}

// N11: "ref-server 01:30 · 5 sessions running". The backend keeps running while windows come
// and go; the timer follows the take's clock and jumps where the film skips time.
export function serverPill(options: ServerPillOptions): Part {
  const el = element('div', 'part server-pill')
  element('span', 'server-pill-dot', el)
  element('span', 'server-pill-name', el).textContent = 'ref-server'
  const timer = element('span', 'server-pill-timer', el)
  const sessions = options.sessions && element('span', 'server-pill-sessions', el)
  if (sessions) sessions.textContent = tr(options.sessions!.text)

  return {
    el,
    render(time) {
      const skipped = (options.jumps ?? []).filter((jump) => time >= jump.at).reduce((sum, jump) => sum + jump.seconds, 0)
      timer.textContent = clock(options.timerStart + Math.max(0, time - options.from) + skipped)
      if (sessions) sessions.style.display = time >= options.sessions!.from ? '' : 'none'
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
    },
  }
}
