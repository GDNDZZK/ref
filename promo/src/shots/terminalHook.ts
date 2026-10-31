// Fake-terminal shots (N1) for the hooks: the situation a viewer knows, drawn with no product.
import type { Rect } from '../components/appFrame.ts'
import { fakeTerminal, type FakeTerminalOptions } from '../components/fakeTerminal.ts'
import type { ShotBuilder } from '../engine/shot.ts'
import { drawnCut, type Cut } from '../films/timeline.ts'

// Where the hook terminals sit on the stage, above the caption band.
export const HOOK_RECT: Rect = { x: 220, y: 90, width: 1480, height: 780 }

// The terminal is on screen from the first frame; the first cut of a film has no entrance.
const SHOWN_BEFORE_START = -1

export function terminalCut(
  from: number,
  to: number,
  options: Omit<FakeTerminalOptions, 'rect' | 'from' | 'to'> & {
    rect?: Rect
    // More parts in the same cut, e.g. a window next to the terminal.
    extras?: (shot: ShotBuilder, seconds: number) => void
  },
  enter?: Cut['enter'],
): Cut {
  const { extras, ...terminal } = options
  return drawnCut(
    from,
    to,
    (shot, seconds) => {
      shot.add(fakeTerminal({ rect: HOOK_RECT, ...terminal, from: from === 0 ? SHOWN_BEFORE_START : 0, to: seconds + 1 }))
      extras?.(shot, seconds)
    },
    enter,
  )
}

const WAITED_SECONDS = 13 * 60 + 57

function minutesAndSeconds(total: number): string {
  const whole = Math.floor(total)
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`
}

// A1, B1, C1 shot 1: four agents in four tabs; `fix` has waited for a yes for almost 14 minutes,
// and the counter ticks past 14:00 during the shot.
export function waitingTabsCut(from: number, to: number): Cut {
  return terminalCut(from, to, {
    tabs: [
      { label: 'fix', active: true, alert: true },
      { label: 'test', spinner: true },
      { label: 'docs', spinner: true },
      { label: 'scout', spinner: true },
    ],
    lines: [
      { at: -1, text: '● Edit src/parser.rs', tone: 'accent' },
      { at: -1, text: '  @@ -41,7 +41,7 @@ fn split_fields', tone: 'muted' },
      { at: -1, text: '  -     let end = start + len;', tone: 'error' },
      { at: -1, text: '  +     let end = start + len - 1;', tone: 'good' },
      { at: -1, text: '' },
      { at: -1, text: 'Allow edit src/parser.rs? [y/n] ', tone: 'warn' },
    ],
    status: (time) => `waiting ${minutesAndSeconds(WAITED_SECONDS + Math.max(0, time))}`,
    statusTone: 'warn',
  })
}
