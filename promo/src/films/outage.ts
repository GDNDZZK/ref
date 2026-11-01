// C3 "The API fails, the run waits and retries" (shooting script section 5), mock script `outage` (M5).
import { keycap } from '../components/keycap.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { castWindow } from '../shots/castWindow.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { OUTAGE_CAST, WIDE_TUI } from './shared.ts'
import { cues, drawnCut, endCut, timelineFilm, type Cut } from './timeline.ts'

const HOOK_END = 3.4
const FIRST_RETRY_END = 7.4
const LONG_WAIT_END = 12.4
const CONTINUES_END = 17.4
const STOP_END = 20.9
const FILM_END = 23.4

// Where each shot starts in the take (tui-outage markers, capture/tui.ts). Shot 2: the prompt is
// sent and the first attempt fails. Shot 3 is the end of the 4th wait and runs on into shot 4,
// where the request goes through 1.5 s in (the take's 4th wait ends 19.2 s after its marker).
// Shot 5: a second run, stopped with esc esc.
const SHOT_SECONDS = { firstRetry: 4, longWait: 5 }
const LONG_WAIT_OFFSET = 19.2 - 1.5 - SHOT_SECONDS.longWait
const SHOT_STARTS = {
  firstRetry: { mark: 'enter', offset: 0.2 },
  longWait: { mark: 'attempt4', offset: LONG_WAIT_OFFSET },
  recovered: { mark: 'attempt4', offset: LONG_WAIT_OFFSET + SHOT_SECONDS.longWait },
  stopped: { mark: 'esc', offset: -1.2 },
}
// Take seconds the cut from shot 2 to shot 3 leaves out (marker attempt4 - enter = 13.66 s).
const SKIPPED_SECONDS = 13.66 + SHOT_STARTS.longWait.offset - (SHOT_STARTS.firstRetry.offset + SHOT_SECONDS.firstRetry)
const ESC_PRESSES = [1.2, 1.35]
// Shot 2 shows the whole take with the sidebar; the others the transcript (columns 0-112) larger.
const CLOSEUP = { scale: 1.3, x: 56 / 150, y: 0.3, rows: 23, cols: 112 }
const CLOSEUP_AT = { x: 115, y: 90 }
// The edit and the passing tests are further down, the second run further still.
const RECOVERED_CLOSEUP = { ...CLOSEUP, y: 0.56 }
const STOPPED_CLOSEUP = { ...CLOSEUP, y: 0.62 }

function outageCut(
  from: number,
  to: number,
  castFrom: { mark: string; offset: number },
  draw?: Parameters<typeof drawnCut>[2],
  // null: the whole take.
  closeup: typeof CLOSEUP | null = CLOSEUP,
): Cut {
  const at = closeup ? CLOSEUP_AT : WIDE_TUI
  return drawnCut(from, to, (shot, seconds) => {
    shot.add(castWindow({ ...OUTAGE_CAST, ...WIDE_TUI, ...at, closeup: closeup ?? undefined, castFrom, start: 0, from: -1, to: seconds + 1 }))
    draw?.(shot, seconds)
  })
}

export const scene = timelineFilm({
  id: 'outage',
  cuts: [
    terminalCut(0, HOOK_END, {
      lines: [
        { at: -1, prompt: '$ ', text: 'agent "run the tests and fix the failures"' },
        { at: 0.3, text: 'error: 529 overloaded', tone: 'muted' },
        { at: 0.9, text: 'error: 529 overloaded', tone: 'muted' },
        { at: 1.5, text: 'error: 529 overloaded', tone: 'muted' },
        { at: 2.1, text: 'Run stopped.', tone: 'error' },
      ],
    }),
    outageCut(HOOK_END, FIRST_RETRY_END, SHOT_STARTS.firstRetry, undefined, null),
    outageCut(FIRST_RETRY_END, LONG_WAIT_END, SHOT_STARTS.longWait, (shot) =>
      shot.add(timeSkipChip({ text: `+${Math.round(SKIPPED_SECONDS)} s`, x: 1500, y: 110, from: 0.1, to: 2.6 })),
    ),
    outageCut(LONG_WAIT_END, CONTINUES_END, SHOT_STARTS.recovered, undefined, RECOVERED_CLOSEUP),
    outageCut(
      CONTINUES_END,
      STOP_END,
      SHOT_STARTS.stopped,
      (shot, seconds) => shot.add(keycap({ keys: ['esc', 'esc'], x: 1560, y: 900, presses: ESC_PRESSES, from: 0.4, to: seconds + 1 })),
      STOPPED_CLOSEUP,
    ),
    endCut(STOP_END, FILM_END),
  ],
  captions: cues([
    [0.2, 3.3, { en: 'After a few retries, the run just stops.', zh: '重试几次不成功，长任务就停了。' }],
    [3.6, 7.2, { en: 'ref shows the error and the attempt.', zh: 'ref 会显示报错和第几次重试。' }],
    [7.6, 12.2, { en: 'Each wait is longer than the last.', zh: '每次等待比上一次更久。' }],
    [12.6, 17.2, { en: 'The task continues where it stopped.', zh: '任务从停下的地方接着做。' }],
    [17.6, 20.8, { en: 'Press esc twice to stop retrying.', zh: '连按两次 esc 可以停止重试。' }],
  ]),
  chip: { ...FEATURES.retries, from: HOOK_END + 0.1, to: STOP_END },
})
