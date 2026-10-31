import { lerp, span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'
import type { Rect } from './appFrame.ts'
import { TERMINAL_FONT, TERMINAL_THEME } from './terminalTheme.ts'
import { typedPrefix } from './typedText.ts'
import { TITLE_BAR_HEIGHT_PX, createWindow } from './windowChrome.ts'

export type TerminalTone = 'plain' | 'muted' | 'error' | 'good' | 'warn' | 'accent'

export interface TerminalLine {
  text: string | Localized
  // When the line appears. Streamed lines appear whole; typed lines type themselves from then on.
  at: number
  typed?: boolean
  charsPerSecond?: number
  // Shown whole before the typed part, e.g. "$ ".
  prompt?: string
  tone?: TerminalTone
}

export interface TerminalTab {
  label: string
  active?: boolean
  // A working agent: a spinner after the label.
  spinner?: boolean
  // A tab that waits for you: a coloured dot after the label.
  alert?: boolean
}

export interface FakeTerminalOptions {
  rect: Rect
  title?: string
  tabs?: readonly TerminalTab[]
  lines: readonly TerminalLine[]
  // Bottom status line, as a function of time so it can count.
  status?: (time: number) => string
  statusTone?: TerminalTone
  fontSize?: number
  // First shown line at `time`. Without it the view follows the newest line.
  scroll?: (time: number) => number
  // The window shrinks to its centre in CLOSE_SECONDS from here, then the frame goes black.
  closeAt?: number
  from: number
  to: number
}

const DEFAULT_FONT_PX = 30
const LINE_HEIGHT = 1.45
const BODY_PADDING_PX = 22
const TAB_BAR_PX = 56
const STATUS_BAR_PX = 48
const DEFAULT_CHARS_PER_SECOND = 22
const SPINNER_FRAMES = '⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
const SPINNER_FPS = 12
const CARET_BLINK_SECONDS = 1
const CLOSE_SECONDS = 0.4
const BLACKOUT_SECONDS = 0.15
const ENTER_SECONDS = 0.6
const EXIT_SECONDS = 0.3
const ENTER_SCALE = 0.96

const TONE_COLORS: Record<TerminalTone, string> = {
  plain: TERMINAL_THEME.foreground!,
  muted: TERMINAL_THEME.brightBlack!,
  error: TERMINAL_THEME.red!,
  good: TERMINAL_THEME.green!,
  warn: TERMINAL_THEME.yellow!,
  accent: TERMINAL_THEME.blue!,
}

function typingSeconds(line: TerminalLine): number {
  return line.typed ? Array.from(tr(line.text)).length / (line.charsPerSecond ?? DEFAULT_CHARS_PER_SECOND) : 0
}

// A dark terminal window with no product on it: tabs, lines that stream or type, a caret,
// a status line. The whole stage is the part, so the close animation can end in black.
export function fakeTerminal(options: FakeTerminalOptions): Part {
  const el = element('div', 'part fake-terminal-stage')
  const { frame, body } = createWindow(options.title ?? '')
  frame.classList.add('fake-terminal')
  el.append(frame)
  const { rect } = options
  Object.assign(frame.style, { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.width}px`, height: `${rect.height}px` })
  const fontSize = options.fontSize ?? DEFAULT_FONT_PX
  body.style.font = `${fontSize}px/${LINE_HEIGHT} ${TERMINAL_FONT}`
  body.style.background = TERMINAL_THEME.background!
  body.style.color = TERMINAL_THEME.foreground!

  const tabBar = options.tabs ? element('div', 'fake-terminal-tabs', body) : undefined
  const tabs = (options.tabs ?? []).map((tab) => {
    const tabEl = element('div', `fake-terminal-tab${tab.active ? ' is-active' : ''}`, tabBar)
    element('span', '', tabEl).textContent = tab.label
    const mark = element('span', `fake-terminal-tab-mark${tab.alert ? ' is-alert' : ''}`, tabEl)
    return { tab, mark }
  })
  const view = element('div', 'fake-terminal-view', body)
  const status = options.status ? element('div', 'fake-terminal-status', body) : undefined
  if (status) status.style.color = TONE_COLORS[options.statusTone ?? 'muted']

  const tabsHeight = tabBar ? TAB_BAR_PX : 0
  const statusHeight = status ? STATUS_BAR_PX : 0
  const viewHeight = rect.height - TITLE_BAR_HEIGHT_PX - tabsHeight - statusHeight - 2 * BODY_PADDING_PX
  const rows = Math.max(1, Math.floor(viewHeight / (fontSize * LINE_HEIGHT)))

  const rowEls = options.lines.map((line) => {
    const row = element('div', 'fake-terminal-line', view)
    row.style.color = TONE_COLORS[line.tone ?? 'plain']
    return row
  })
  const caret = element('span', 'fake-terminal-caret')
  const blackout = element('div', 'fake-terminal-blackout', el)

  return {
    el,
    render(time) {
      const shown = options.lines.flatMap((line, index) => (time >= line.at ? [index] : []))
      const first = options.scroll ? Math.floor(options.scroll(time)) : Math.max(0, shown.length - rows)
      let typing = false
      options.lines.forEach((line, index) => {
        const row = rowEls[index]!
        const visible = time >= line.at && index >= first && index < first + rows
        row.style.display = visible ? '' : 'none'
        const text = tr(line.text)
        const typed = line.typed ? typedPrefix(text, time - line.at, line.charsPerSecond ?? DEFAULT_CHARS_PER_SECOND) : text
        if (line.typed && time < line.at + typingSeconds(line)) typing = true
        row.textContent = `${line.prompt ?? ''}${typed}` || '\u00a0'
      })
      const last = shown.at(-1)
      const caretOn = typing || Math.floor((time / CARET_BLINK_SECONDS) * 2) % 2 === 0
      caret.style.visibility = caretOn ? '' : 'hidden'
      if (last !== undefined) rowEls[last]!.append(caret)

      const spinner = SPINNER_FRAMES[Math.floor(time * SPINNER_FPS) % SPINNER_FRAMES.length]!
      for (const { tab, mark } of tabs) mark.textContent = tab.spinner ? spinner : tab.alert ? '●' : ''
      if (status) status.textContent = options.status!(time)

      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      const closing = options.closeAt === undefined ? 0 : span(time, options.closeAt, CLOSE_SECONDS, 'inCubic')
      frame.style.opacity = String(entering * (1 - leaving) * (1 - closing))
      frame.style.transform = `scale(${lerp(ENTER_SCALE, 1, entering) * (1 - closing)})`
      const dark = options.closeAt === undefined ? 0 : span(time, options.closeAt + CLOSE_SECONDS - BLACKOUT_SECONDS, BLACKOUT_SECONDS)
      blackout.style.opacity = String(dark)
    },
  }
}
