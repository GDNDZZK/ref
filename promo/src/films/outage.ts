// C3 "The API fails, the run waits and retries" (shooting script section 5), mock script `outage` (M5).
import { keycap } from '../components/keycap.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { castWindow } from '../shots/castWindow.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, drawnCut, endCut, timelineFilm, type Cut } from './timeline.ts'

const HOOK_END = 3.4
const FIRST_RETRY_END = 7.4
const LONG_WAIT_END = 12.4
const CONTINUES_END = 17.4
const STOP_END = 20.9
const FILM_END = 23.4

// Cast seconds where each shot of the take starts (tui-outage, measured on the recording).
const CAST_AT = { firstRetry: 6, longWait: 16, recovered: 30, retryAgain: 8 }
const CAST_FONT_PX = 22
const CAST_POSITION = { x: 200, y: 96 }

function outageCut(from: number, to: number, castFrom: number, draw?: Parameters<typeof drawnCut>[2]): Cut {
  return drawnCut(from, to, (shot, seconds) => {
    shot.add(castWindow({ name: 'tui-outage', title: 'ref', ...CAST_POSITION, fontSize: CAST_FONT_PX, hideLastRows: 0, start: -castFrom, from: -1, to: seconds + 1 }))
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
    outageCut(HOOK_END, FIRST_RETRY_END, CAST_AT.firstRetry),
    outageCut(FIRST_RETRY_END, LONG_WAIT_END, CAST_AT.longWait, (shot) =>
      shot.add(timeSkipChip({ text: '+30 s', x: 1560, y: 120, from: 0.1, to: 2.6 })),
    ),
    outageCut(LONG_WAIT_END, CONTINUES_END, CAST_AT.recovered),
    outageCut(CONTINUES_END, STOP_END, CAST_AT.retryAgain, (shot) =>
      shot.add(keycap({ keys: ['esc', 'esc'], x: 960, y: 760, presses: [1.2, 1.5], from: 0.4, to: STOP_END - CONTINUES_END + 1 })),
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
