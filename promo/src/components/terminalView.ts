import { Terminal } from '@xterm/xterm'
import { lerp, span } from '../engine/anim.ts'
import type { Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'
import { highlight } from './highlight.ts'
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
  // Only this many rows from the top are in view (a shell take that prints a few lines).
  topRows?: number
  // Space inside the window around the screen, in pixels.
  padding?: number
  // A close-up: the screen scaled up so that a point of it (fractions of the screen) is in the
  // middle of the window; with `rows` (`cols`) the window is only that many scaled rows high
  // (columns wide).
  closeup?: { scale: number; x: number; y: number; rows?: number; cols?: number }
  // Starts as the whole screen with its top-left corner at `wide`, then zooms into the
  // close-up (at `x`, `y`) from `at` on.
  zoomIn?: { at: number; wide: { x: number; y: number } }
  // Callouts on screen cells: from column `col` of row `row` (0-based), `cols` wide.
  marks?: readonly CellMark[]
  // Cast seconds that pass per video second.
  speed?: number
  // Video time at which the cast starts playing: from its beginning, or from `castFrom`.
  start: number
  // Plays from a marker of the cast, `offset` seconds after it (negative: before).
  castFrom?: { mark: string; offset?: number }
  from: number
  to: number
}

export interface CellMark {
  col: number
  row: number
  cols: number
  rows?: number
  label?: string | Localized
  from: number
  to: number
}

// Room around the marked cells, in cells.
const MARK_MARGIN = { cols: 0.6, rows: 0.2 }

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

// Where the window is and how its screen sits in it: stage pixels; host offset and scale.
interface View {
  x: number
  y: number
  width: number
  height: number
  hostX: number
  hostY: number
  scale: number
}

const ZOOM_SECONDS = 0.9

function blend(from: View, to: View, progress: number): View {
  const mix = (key: keyof View) => lerp(from[key], to[key], progress)
  return { x: mix('x'), y: mix('y'), width: mix('width'), height: mix('height'), hostX: mix('hostX'), hostY: mix('hostY'), scale: mix('scale') }
}

// A terminal window replaying a recorded session. Seeking backwards replays from the start.
export function terminalView(options: TerminalViewOptions): Part {
  const { frame, body } = createWindow(options.title)
  const host = element('div', 'terminal-host', body)

  const speed = options.speed ?? 1
  let ready: Promise<{ terminal: Terminal; cast: Cast; wide: View; close: View; marks: Part[] }> | undefined
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
    const width = screen.offsetWidth
    const rowHeight = screen.offsetHeight / cast.rows
    const shownRows = options.topRows ?? cast.rows - (options.hideLastRows ?? 0)
    const padding = options.padding ?? 0
    if (padding && options.closeup) throw new Error(`${options.cast}: padding does not work with a closeup`)
    body.style.padding = `${padding}px`
    host.style.transformOrigin = '0 0'
    host.style.position = 'relative'
    const marks = (options.marks ?? []).map((mark) => {
      const part = highlight({
        x: (mark.col - MARK_MARGIN.cols) / cast.cols,
        y: (mark.row - MARK_MARGIN.rows) / cast.rows,
        width: (mark.cols + 2 * MARK_MARGIN.cols) / cast.cols,
        height: ((mark.rows ?? 1) + 2 * MARK_MARGIN.rows) / cast.rows,
        label: mark.label,
        from: mark.from,
        to: mark.to,
      })
      host.append(part.el)
      return part
    })
    const wideAt = options.zoomIn?.wide ?? options
    const wide: View = { x: wideAt.x, y: wideAt.y, width: width + 2 * padding, height: rowHeight * shownRows + 2 * padding, hostX: 0, hostY: 0, scale: 1 }
    const closeup = options.closeup
    if (!closeup) return { terminal, cast, wide, close: wide, marks }
    const viewWidth = closeup.cols ? closeup.cols * (width / cast.cols) * closeup.scale : width
    const viewHeight = closeup.rows ? closeup.rows * rowHeight * closeup.scale : rowHeight * shownRows
    const clamp = (offset: number, size: number, view: number) => Math.min(0, Math.max(view - size * closeup.scale, offset))
    const close: View = {
      x: options.x,
      y: options.y,
      width: viewWidth,
      height: viewHeight,
      hostX: clamp(viewWidth / 2 - closeup.x * width * closeup.scale, width, viewWidth),
      hostY: clamp(viewHeight / 2 - closeup.y * screen.offsetHeight * closeup.scale, rowHeight * shownRows, viewHeight),
      scale: closeup.scale,
    }
    return { terminal, cast, wide, close, marks }
  }

  return {
    el: frame,
    async render(time) {
      ready ??= open()
      const { terminal, cast, wide, close, marks } = await ready

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

      const zoomed = options.zoomIn ? span(time, options.zoomIn.at, ZOOM_SECONDS, 'inOutSine') : 1
      const view = blend(wide, close, zoomed)
      Object.assign(frame.style, { left: `${view.x}px`, top: `${view.y}px`, width: `${view.width}px`, height: `${view.height + TITLE_BAR_HEIGHT_PX}px` })
      host.style.transform = `translate(${view.hostX}px, ${view.hostY}px) scale(${view.scale})`
      for (const mark of marks) mark.render(time)

      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      frame.style.opacity = String(entering * (1 - leaving))
      frame.style.transform = `scale(${lerp(ENTER_SCALE, 1, entering)})`
    },
  }
}
