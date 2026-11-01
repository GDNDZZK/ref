// C8 "Run the backend yourself" (shooting script section 5): a standalone ref-server (RB2) and the
// mock script `longrun` (M3).
import { serverPill } from '../components/serverPill.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { windowClose } from '../components/windowClose.ts'
import { castWindow } from '../shots/castWindow.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { SERVER_CAST } from './shared.ts'
import { cues, drawnCut, endCut, timelineFilm } from './timeline.ts'

const HOOK_END = 3.4
const ATTACH_END = 7.4
const CLOSE_END = 11.4
const SKIP_END = 13.9
const REOPEN_END = 18.9
const FILM_END = 21.4

const HOOK_CLOSE_AT = 2.2

// RB2 from just before `ref-server` is typed; the attached `ref` (tui-longrun) comes in front
// once the server listens and types the prompt.
const SERVER_TYPED = { mark: 'typed', offset: -0.2 }
const REF_IN_AT = 1.6
const PROMPT_TYPED = { mark: 'typed', offset: -0.2 }
// Shot 3: step 6/20 prints, then the window closes.
const STEP6_LEAD = 1.2
const CLOSE_AT = 2
const BEFORE_STEP6 = { mark: 'step6', offset: -STEP6_LEAD }
// Shot 5 (tui-longrun-reopen): ref starts with the running session in its sidebar, then a cut
// to the sessions dialog just before Enter opens it; step 18/20 prints live about 2.3 s later.
const REOPEN_READY = { mark: 'ready', offset: -0.3 }
const DIALOG_CUT = 1.5
const BEFORE_OPEN = { mark: 'opened', offset: -0.6 }

// The take's clock (the mock backend starts with the first ref, so its uptime is cast time of
// tui-longrun). Marker step6 = 55.1 s. The session is opened 116.7 s after the close (step 18
// prints at 175.1 s in that clock, 2.34 s after the opened marker).
const STEP6_UPTIME = 55.1
const OPENED_UPTIME = 175.1 - 2.34
const REOPEN_READY_UPTIME = OPENED_UPTIME - (6.63 - 2.08) - 0.3
const SKIP_AT = CLOSE_END + 0.6

// The ref window sits below the server's two lines, which stay readable above it.
const REF_WINDOW = { name: 'tui-longrun', title: 'ref', x: 240, y: 236, fontSize: 16, hideLastRows: 1 }
const REOPEN_WINDOW = { ...REF_WINDOW, name: 'tui-longrun-reopen' }
const SERVER_WINDOW = { ...SERVER_CAST, topRows: 3, x: 120, y: 84 }
// The step 18/20 row of the reopened session, once it prints (opened + 2.34 s).
const STEP18_MARK = { row: 29, col: 7, cols: 10 }
const STEP18_SHOWN = 0.6 + 2.4

// `start`: cut time at which the server take is at its start, so it runs on across cuts.
function serverWindow(seconds: number, from: number, start: number) {
  return castWindow({ ...SERVER_WINDOW, castFrom: SERVER_TYPED, start, from, to: seconds + 1 })
}

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
      shot.add(serverWindow(seconds, 0, 0))
      shot.add(castWindow({ ...REF_WINDOW, castFrom: PROMPT_TYPED, start: REF_IN_AT, from: REF_IN_AT, to: seconds + 1 }))
    }),
    drawnCut(ATTACH_END, CLOSE_END, (shot, seconds) => {
      shot.add(serverWindow(seconds, -1, HOOK_END - ATTACH_END))
      shot.add(windowClose(castWindow({ ...REF_WINDOW, castFrom: BEFORE_STEP6, start: 0, from: -1, to: seconds + 1 }), CLOSE_AT))
    }),
    drawnCut(CLOSE_END, SKIP_END, (shot, seconds) => {
      shot.add(serverWindow(seconds, -1, HOOK_END - CLOSE_END))
      shot.add(timeSkipChip({ text: '+2 min', x: 880, y: 520, from: 0.2, to: seconds }))
    }),
    drawnCut(SKIP_END, SKIP_END + DIALOG_CUT, (shot, seconds) =>
      shot.add(castWindow({ ...REOPEN_WINDOW, castFrom: REOPEN_READY, start: 0, from: 0, to: seconds + 1 })),
    ),
    drawnCut(
      SKIP_END + DIALOG_CUT,
      REOPEN_END,
      (shot, seconds) =>
        shot.add(
          castWindow({
            ...REOPEN_WINDOW,
            castFrom: BEFORE_OPEN,
            marks: [{ ...STEP18_MARK, from: STEP18_SHOWN, to: seconds + 1 }],
            start: 0,
            from: -1,
            to: seconds + 1,
          }),
        ),
      'cut',
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
        timerStart: STEP6_UPTIME - STEP6_LEAD,
        // The cut to the reopened ref lands on its clock.
        jumps: [{ at: SKIP_AT, seconds: REOPEN_READY_UPTIME - (STEP6_UPTIME - STEP6_LEAD) - (SKIP_END - ATTACH_END) }],
        from: ATTACH_END,
        to: REOPEN_END,
      }),
    ),
})
