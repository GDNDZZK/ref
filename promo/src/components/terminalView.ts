import { Terminal } from '@xterm/xterm'
import { lerp, span } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'
import { TERMINAL_FONT, TERMINAL_THEME } from './terminalTheme.ts'
import { TITLE_BAR_HEIGHT_PX, createWindow } from './windowChrome.ts'

interface CastEvent {
  time: number
  data: string
}

interface Cast {
  cols: number
  rows: number
  events: CastEvent[]
  // Markers the capture set (capture/lib/tuiSession.ts mark), seconds into the cast.
  marks: Map<string, number>
}

export interface TerminalViewOptions {
  // URL of an asciicast v2 file recorded by capture/tui.ts.
  cast: string
  title: string
  x: number
  y: number
  fontSize: number
  // Rows at the bottom that stay out of view (the status line shows the random mock path).
  hideLastRows?: number
  // Cast seconds that pass per video second.
  speed?: number
  // Video time at which the cast starts playing: from its beginning, or from `castFrom`.
  start: number
  // Plays from a marker of the cast, `offset` seconds after it (negative: before).
  castFrom?: { mark: string; offset?: number }
  from: number
  to: number
}

const ENTER_SECONDS = 0.8
const EXIT_SECONDS = 0.35
const ENTER_SCALE = 0.94

// asciicast v2: a JSON header line, then [seconds, "o", text] output and [seconds, "m", label] markers.
async function loadCast(url: string): Promise<Cast> {
  const text = await (await fetch(url)).text()
  const [headerLine, ...eventLines] = text.split('\n').filter((line) => line.length > 0)
  const header = JSON.parse(headerLine!) as { width: number; height: number }
  const events: CastEvent[] = []
  const marks = new Map<string, number>()
  for (const line of eventLines) {
    const [time, kind, data] = JSON.parse(line) as [number, string, string]
    if (kind === 'o') events.push({ time, data })
    else if (kind === 'm') marks.set(data, time)
  }
  return { cols: header.width, rows: header.height, events, marks }
}

function castStart(cast: Cast, castFrom: TerminalViewOptions['castFrom'], url: string): number {
  if (!castFrom) return 0
  const mark = cast.marks.get(castFrom.mark)
  if (mark === undefined) throw new Error(`the cast ${url} has no marker "${castFrom.mark}"`)
  return mark + (castFrom.offset ?? 0)
}

function nextFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()))
}

// A terminal window replaying a recorded session. Seeking backwards replays from the start.
export function terminalView(options: TerminalViewOptions): Part {
  const { frame, body } = createWindow(options.title)
  frame.style.left = `${options.x}px`
  frame.style.top = `${options.y}px`
  const host = element('div', 'terminal-host', body)

  const speed = options.speed ?? 1
  let ready: Promise<{ terminal: Terminal; cast: Cast }> | undefined
  let appliedEvents = 0

  const open = async () => {
    const cast = await loadCast(options.cast)
    const terminal = new Terminal({
      cols: cast.cols,
      rows: cast.rows,
      fontFamily: TERMINAL_FONT,
      fontSize: options.fontSize,
      cursorBlink: false,
      disableStdin: true,
      theme: TERMINAL_THEME,
    })
    terminal.open(host)
    await nextFrame()
    const screen = host.querySelector<HTMLElement>('.xterm-screen')!
    frame.style.width = `${screen.offsetWidth}px`
    const shownRows = cast.rows - (options.hideLastRows ?? 0)
    frame.style.height = `${(screen.offsetHeight * shownRows) / cast.rows + TITLE_BAR_HEIGHT_PX}px`
    return { terminal, cast }
  }

  return {
    el: frame,
    async render(time) {
      ready ??= open()
      const { terminal, cast } = await ready

      const castTime = castStart(cast, options.castFrom, options.cast) + Math.max(0, time - options.start) * speed
      const wanted = cast.events.filter((event) => event.time <= castTime).length
      if (wanted < appliedEvents) {
        terminal.reset()
        appliedEvents = 0
      }
      if (wanted > appliedEvents) {
        const data = cast.events.slice(appliedEvents, wanted).map((event) => event.data).join('')
        await new Promise<void>((resolve) => terminal.write(data, resolve))
        appliedEvents = wanted
        await nextFrame()
      }

      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      frame.style.opacity = String(entering * (1 - leaving))
      frame.style.transform = `scale(${lerp(ENTER_SCALE, 1, entering)})`
    },
  }
}
