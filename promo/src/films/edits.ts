// C2 "Edits go by line number, with a file tag" (shooting script section 5), mock script `stale` (M4).
// The turn runs in about 4 s with its cards closed; the take then opens them one at a time
// (capture/scenarios.ts `stale`), and each cut here shows one card at its clip mark.
import type { Localized } from '../engine/i18n.ts'
import { clip, type ClipClick } from '../engine/clips.ts'
import { beatShot, type Look, type Mark } from '../shots/beat.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, endCut, timelineFilm, type Cut } from './timeline.ts'

const HOOK_END = 3.0
const READ_END = 7.0
const PULL_END = 10.5
const MOVED_END = 16.0
const REJECTED_END = 19.5
const TAKE_END = 22.5
const FILM_END = 25.0

const STALE_CLIP = 'gui-stale'
// Seconds of the clip shown before a card is clicked open, and how long an open card is shown
// before the take closes it again.
const BEFORE_CLICK_SECONDS = 0.7
const OPEN_SECONDS = 2.4
const OPENED_SECONDS = 0.5 // the card has finished opening
const POINTER_LEAD_SECONDS = 0.7
const POINTER_START_OFFSET = { x: 0.12, y: 0.16 }

// Picture coordinates (0..1) of the parts the film points at, read off the take.
const TAG = { x: 0.31, y: 0.235, width: 0.145, height: 0.03 }
const READ_LINE_12 = { x: 0.315, y: 0.522, width: 0.26, height: 0.027 }
const PULL_OUTPUT = { x: 0.31, y: 0.525, width: 0.08, height: 0.03 }
const MOVED_HUNK = { x: 0.315, y: 0.615, width: 0.11, height: 0.027 }
const MOVED_LINES = { x: 0.31, y: 0.705, width: 0.58, height: 0.05 }
const REJECTION = { x: 0.325, y: 0.322, width: 0.47, height: 0.03 }
const CURRENT_LINE_12 = { x: 0.31, y: 0.203, width: 0.32, height: 0.03 }
const RETRIED_USE = { x: 0.31, y: 0.474, width: 0.6, height: 0.046 }
const RETRIED_LINE_12 = { x: 0.31, y: 0.655, width: 0.6, height: 0.046 }

// The edit cards show the absolute path of the mock workspace above their diff, so the camera
// stays below that row (MOVED_VIEW top 0.57, RETRIED_VIEW top 0.335).
const READ_VIEW = { x: 0.48, y: 0.4, zoom: 1.6 }
const PULL_VIEW = { x: 0.5, y: 0.45, zoom: 1.8 }
const MOVED_VIEW = { x: 0.45, y: 0.77, zoom: 2.5 }
const REJECTED_VIEW = { x: 0.55, y: 0.32, zoom: 1.8 }
const CURRENT_VIEW = { x: 0.5, y: 0.25, zoom: 1.8 }
const RETRIED_VIEW = { x: 0.45, y: 0.56, zoom: 2.2 }
const PUSH_IN = 1.08 // a slow zoom while a still frame is held

const MOVED_LABEL: Localized = { en: 'read at line 12, now line 15', zh: '读取时在第 12 行，现在在第 15 行' }

function staleTake() {
  const recorded = clip(STALE_CLIP)
  const mark = (name: string): number => {
    const at = recorded.marks[name]
    if (at === undefined) throw new Error(`${STALE_CLIP} lacks the mark ${name}`)
    return at
  }
  // The click that opened the card at `name` (the take marks it right after clicking).
  const clickAt = (name: string): ClipClick => {
    const at = mark(name)
    return recorded.clicks.reduce((best, click) => (Math.abs(click.at - at) < Math.abs(best.at - at) ? click : best))
  }
  return { mark, clickAt }
}

function pushIn(view: Omit<Look, 'at'>, seconds: number): Look[] {
  return [
    { at: 0, ...view },
    { at: seconds, ...view, zoom: view.zoom * PUSH_IN },
  ]
}

function shown(area: Omit<Mark, 'from' | 'to'>, from: number, to: number, label?: Localized): Mark {
  return { ...area, from, to, label }
}

// A cut in which the take clicks one card open: the clip runs from just before the click and
// holds once the card has been open for OPEN_SECONDS.
function openingCut(from: number, to: number, markName: string, look: Omit<Look, 'at'>, marks: (clickAt: number, seconds: number) => Mark[]): Cut {
  return {
    from,
    to,
    build: (seconds) => {
      const take = staleTake()
      const opened = take.mark(markName)
      const click = take.clickAt(markName)
      const start = opened - BEFORE_CLICK_SECONDS
      const clickAt = click.at - start
      return beatShot({
        seconds,
        screens: [{ clip: STALE_CLIP, time: (time) => start + Math.min(time, BEFORE_CLICK_SECONDS + OPEN_SECONDS) }],
        looks: pushIn(look, seconds),
        marks: marks(clickAt, seconds),
        pointer: {
          path: [
            { at: clickAt - POINTER_LEAD_SECONDS, x: click.x + POINTER_START_OFFSET.x, y: click.y + POINTER_START_OFFSET.y },
            { at: clickAt, x: click.x, y: click.y },
          ],
          clicks: [clickAt],
        },
        windowFrom: from === HOOK_END ? 0 : -1,
      })
    },
  }
}

// A cut that holds one frame of the take: the card is open, its header out of view.
function heldCut(from: number, to: number, markName: string, look: Omit<Look, 'at'>, marks: Mark[]): Cut {
  return {
    from,
    to,
    build: (seconds) => {
      const held = staleTake().mark(markName) + OPENED_SECONDS
      return beatShot({ seconds, screens: [{ clip: STALE_CLIP, time: () => held }], looks: pushIn(look, seconds), marks, windowFrom: -1 })
    },
  }
}

// The rejected card: first closed, with the rejection as its first output line, then clicked
// open on the current lines.
const rejectedCut: Cut = {
  from: MOVED_END,
  to: REJECTED_END,
  build: (seconds) => {
    const take = staleTake()
    const opened = take.mark('rejectedOpen')
    const click = take.clickAt('rejectedOpen')
    const closedFor = 1.8
    const clickAt = closedFor + (click.at - opened)
    const closedFrame = click.at - 0.1
    return beatShot({
      seconds,
      screens: [{ clip: STALE_CLIP, time: (time) => (time < closedFor ? closedFrame : Math.min(closedFrame + time - closedFor, opened + OPEN_SECONDS)) }],
      looks: [
        { at: 0, ...REJECTED_VIEW },
        { at: closedFor, ...REJECTED_VIEW },
        { at: closedFor + 0.5, ...CURRENT_VIEW, ease: 'inOutSine' },
      ],
      marks: [shown(REJECTION, 0.3, closedFor), shown(CURRENT_LINE_12, closedFor + 0.6, seconds)],
      pointer: {
        path: [
          { at: clickAt - POINTER_LEAD_SECONDS, x: click.x + POINTER_START_OFFSET.x, y: click.y + POINTER_START_OFFSET.y },
          { at: clickAt, x: click.x, y: click.y },
        ],
        clicks: [clickAt],
      },
      windowFrom: -1,
    })
  },
}

export const scene = timelineFilm({
  id: 'edits',
  cuts: [
    // The parser as the take reads it: `parse_line` on line 12; three lines land on top of it.
    terminalCut(0, HOOK_END, {
      lines: [
        { at: -1, prompt: '$ ', text: 'git pull' },
        { at: -1, text: ' scratch/src/parser.rs | 3 +++', tone: 'muted' },
        { at: -1, text: ' 1 file changed, 3 insertions(+)', tone: 'muted' },
        { at: 0.5, text: '' },
        { at: 0.5, text: '● edit scratch/src/parser.rs, line 12', tone: 'accent' },
        { at: 1.0, text: '  read:  12  pub fn parse_line(text: &str) -> Option<Row> {', tone: 'muted' },
        { at: 1.5, text: '  now:   12  (empty line)', tone: 'error' },
        { at: 1.9, text: '         15  pub fn parse_line(text: &str) -> Option<Row> {', tone: 'muted' },
      ],
    }),
    openingCut(HOOK_END, READ_END, 'readOpen', READ_VIEW, (clickAt, seconds) => [shown(TAG, clickAt + 0.6, seconds), shown(READ_LINE_12, clickAt + 1.4, seconds)]),
    openingCut(READ_END, PULL_END, 'pullOpen', PULL_VIEW, (clickAt, seconds) => [shown(PULL_OUTPUT, clickAt + 0.6, seconds)]),
    heldCut(MOVED_END - 5.5, MOVED_END, 'movedOpen', MOVED_VIEW, [shown(MOVED_HUNK, 0.6, 5.5), shown(MOVED_LINES, 1.4, 5.5, MOVED_LABEL)]),
    rejectedCut,
    heldCut(REJECTED_END, TAKE_END, 'retriedDiff', RETRIED_VIEW, [shown(RETRIED_USE, 0.5, 3), shown(RETRIED_LINE_12, 0.9, 3)]),
    endCut(TAKE_END, FILM_END),
  ],
  captions: cues([
    [0.2, 2.9, { en: 'Line 12 is no longer the line you read.', zh: '第 12 行已经不是你读到的那一行了。' }],
    [3.2, 6.8, { en: 'ref edits by line. Every read returns a tag.', zh: 'ref 按行号改，读文件都带标签。' }],
    [7.2, 10.3, { en: 'Then the file changes on disk.', zh: '然后磁盘上的文件被改了。' }],
    [10.7, 15.8, { en: 'A shifted file: ref moves the edit.', zh: '文件移位了，ref 会把修改对过去。' }],
    [16.2, 19.4, { en: 'If those lines changed, ref rejects the edit.', zh: '改的那几行变了，ref 就拒绝。' }],
    [19.7, 22.4, { en: 'The model retries with the current lines.', zh: '模型按最新内容重试。' }],
  ]),
  chip: { ...FEATURES.edits, from: HOOK_END + 0.1, to: TAKE_END },
})
