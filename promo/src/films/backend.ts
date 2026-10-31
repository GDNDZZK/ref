// C8 "Run the backend yourself" (shooting script section 5): a standalone ref-server (RB2) and the
// mock script `longrun` (M3).
import { serverPill } from '../components/serverPill.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { windowClose } from '../components/windowClose.ts'
import { castWindow } from '../shots/castWindow.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, drawnCut, endCut, timelineFilm } from './timeline.ts'

const HOOK_END = 3.4
const ATTACH_END = 7.4
const CLOSE_END = 11.4
const SKIP_END = 13.9
const REOPEN_END = 18.9
const FILM_END = 21.4

const HOOK_CLOSE_AT = 2.2
// Cast seconds: where the server start begins, where the attached `ref` types the prompt, where
// the window is closed at step 6/20, and where the reopened `ref` shows the running session.
const CAST_AT = { serverStart: 0, attach: 0, closeAtStep6: 60, reopen: 0 }
// The take's pill clock: 01:30 when the window closes; the gap is 120 s.
const PILL_AT_CLOSE_SECONDS = 90
const GAP_SECONDS = 120
const CLOSE_AT = 2

const SERVER_WINDOW = { x: 120, y: 110, fontSize: 20 }
const REF_WINDOW = { x: 520, y: 260, fontSize: 20 }

export const scene = timelineFilm({
  id: 'backend',
  cuts: [
    terminalCut(0, HOOK_END, {
      lines: [
        { at: -1, prompt: '$ ', text: 'agent "run every test in src/"' },
        { at: -1, text: 'step 4/20: tests::parser_04 ... ok', tone: 'muted' },
        { at: 0.2, text: 'step 5/20: tests::parser_05 ... ok', tone: 'muted' },
        { at: 1.2, text: 'step 6/20: tests::parser_06 ... ok', tone: 'muted' },
      ],
      closeAt: HOOK_CLOSE_AT,
    }),
    drawnCut(HOOK_END, ATTACH_END, (shot, seconds) => {
      shot.add(castWindow({ name: 'tui-server-start', title: 'ref-server', ...SERVER_WINDOW, start: -CAST_AT.serverStart, from: 0, to: seconds + 1, pendingSize: { width: 1100, height: 400 } }))
      shot.add(castWindow({ name: 'tui-longrun', title: 'ref', ...REF_WINDOW, start: 1.4 - CAST_AT.attach, from: 1.4, to: seconds + 1, pendingSize: { width: 1280, height: 640 } }))
    }),
    drawnCut(CLOSE_END - 4, CLOSE_END, (shot, seconds) => {
      shot.add(castWindow({ name: 'tui-server-start', title: 'ref-server', ...SERVER_WINDOW, start: -CAST_AT.serverStart, from: -1, to: seconds + 1, pendingSize: { width: 1100, height: 400 } }))
      const ref = castWindow({ name: 'tui-longrun', title: 'ref', ...REF_WINDOW, start: CLOSE_AT - CAST_AT.closeAtStep6, from: -1, to: seconds + 1, pendingSize: { width: 1280, height: 640 } })
      shot.add(windowClose(ref, CLOSE_AT))
    }),
    drawnCut(CLOSE_END, SKIP_END, (shot, seconds) => {
      shot.add(castWindow({ name: 'tui-server-start', title: 'ref-server', ...SERVER_WINDOW, start: -CAST_AT.serverStart, from: -1, to: seconds + 1, pendingSize: { width: 1100, height: 400 } }))
      shot.add(timeSkipChip({ text: '+2 min', x: 880, y: 520, from: 0.2, to: seconds }))
    }),
    drawnCut(SKIP_END, REOPEN_END, (shot, seconds) =>
      shot.add(castWindow({ name: 'tui-longrun-reopen', title: 'ref', ...REF_WINDOW, start: -CAST_AT.reopen, from: 0, to: seconds + 1, pendingSize: { width: 1280, height: 640 } })),
    ),
    endCut(REOPEN_END, FILM_END),
  ],
  captions: cues([
    [0.2, 3.3, { en: 'Closing the terminal stops the run.', zh: '关掉终端，运行就停了。' }],
    [3.6, 7.2, { en: 'Unless the backend runs on its own.', zh: '除非后端单独运行。' }],
    [7.6, 11.2, { en: 'Close the window. The task keeps running.', zh: '关掉窗口，任务继续跑。' }],
    [11.5, 13.8, { en: 'Two minutes later.', zh: '两分钟后。' }],
    [14.1, 18.7, { en: 'Open ref again. Output from the gap is there.', zh: '再开 ref，这段时间的输出都在。' }],
  ]),
  chip: { ...FEATURES.backend, from: HOOK_END + 0.1, to: REOPEN_END },
  overlay: (shot) =>
    shot.add(
      serverPill({
        timerStart: PILL_AT_CLOSE_SECONDS - CLOSE_AT,
        jumps: [{ at: CLOSE_END + 0.6, seconds: GAP_SECONDS }],
        from: ATTACH_END,
        to: REOPEN_END,
      }),
    ),
})
