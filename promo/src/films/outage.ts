// C3 "The API fails, the run waits and retries" (shooting script section 5), mock script `outage` (M5).
import { keycap } from '../components/keycap.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { castWindow } from '../shots/castWindow.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { OUTAGE_CAST } from './shared.ts'
import { cues, drawnCut, endCut, timelineFilm, type Cut } from './timeline.ts'

const HOOK_END = 3.4
const FIRST_RETRY_END = 7.4
const LONG_WAIT_END = 12.4
const CONTINUES_END = 17.4
const STOP_END = 20.9
const FILM_END = 23.4

// Where each shot starts in the take (tui-outage markers, capture/tui.ts). Shot 2: the prompt is
// sent and the first attempt fails. Shot 3 is the end of the 4th wait and runs on into shot 4,
// where the request goes through (the take's 4th wait is about 15 s). Shot 5: a second run,
// stopped with esc esc.
const SHOT_SECONDS = { firstRetry: 4, longWait: 5 }
const SHOT_STARTS = {
  firstRetry: { mark: 'enter', offset: 0.2 },
  longWait: { mark: 'attempt4', offset: 9.6 },
  recovered: { mark: 'attempt4', offset: 9.6 + SHOT_SECONDS.longWait },
  stopped: { mark: 'esc', offset: -1.2 },
}
// Take seconds the cut from shot 2 to shot 3 leaves out (marker attempt4 - enter = 14.1 s).
const SKIPPED_SECONDS = 14.1 + SHOT_STARTS.longWait.offset - (SHOT_STARTS.firstRetry.offset + SHOT_SECONDS.firstRetry)
const ESC_PRESSES = [1.2, 1.35]
const CLOSEUP = { scale: 1.15, x: 0.3, y: 0.3, rows: 23 }
// The second run is further down the transcript.
const STOPPED_CLOSEUP = { ...CLOSEUP, y: 0.62 }

function outageCut(
  from: number,
  to: number,
  castFrom: { mark: string; offset: number },
  draw?: Parameters<typeof drawnCut>[2],
  closeup = CLOSEUP,
): Cut {
  return drawnCut(from, to, (shot, seconds) => {
    shot.add(castWindow({ ...OUTAGE_CAST, closeup, castFrom, start: 0, from: -1, to: seconds + 1 }))
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
    outageCut(HOOK_END, FIRST_RETRY_END, SHOT_STARTS.firstRetry),
    outageCut(FIRST_RETRY_END, LONG_WAIT_END, SHOT_STARTS.longWait, (shot) =>
      shot.add(timeSkipChip({ text: `+${Math.round(SKIPPED_SECONDS)} s`, x: 1500, y: 110, from: 0.1, to: 2.6 })),
    ),
    outageCut(LONG_WAIT_END, CONTINUES_END, SHOT_STARTS.recovered),
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
