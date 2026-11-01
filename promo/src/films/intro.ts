// Film A "What is ref" (shooting script section 3). Times are film seconds from the script.
import { compareCard } from '../components/compareCard.ts'
import { fakeTerminal } from '../components/fakeTerminal.ts'
import { releaseCard } from '../components/releaseCard.ts'
import { serverPill } from '../components/serverPill.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { CLOSE_SECONDS } from '../components/windowClose.ts'
import { featureTag } from '../components/featureTag.ts'
import { beatShot } from '../shots/beat.ts'
import { castWindow } from '../shots/castWindow.ts'
import { heroCut } from '../shots/hero.ts'
import { HOOK_RECT, waitingTabsCut } from '../shots/terminalHook.ts'
import { threeWindowsShot } from '../shots/threeWindows.ts'
import { COMPARE } from './compare.ts'
import { COPY, HERO_CAPTIONS, HERO_FILM_START } from './copy.ts'
import { FEATURES } from './features.ts'
import { RELEASE } from './release.ts'
import { FIXTURE_CAST, OUTAGE_CAST, SERVER_CAST, WIDE_TUI, addBrandTitle, clickedClip, crewWindowsCut, opencodeModelMarks } from './shared.ts'
import { cues, drawnCut, endCut, timelineFilm, type Cut } from './timeline.ts'

const A6_END = 24.4
// From after the reviewer's raw message has scrolled away (main's to-do card at the top of the
// tile); the notification lands at film 26.1.
const A7_CLIP_FROM = 23.3
const HERO_END = 29.9
const CARD_END = 38.4
const CREW_END = 43.4
const SERVER_START_END = 45.4
const STANDALONE_END = 49.4
const FEATURE_CUT_SECONDS = 3.4
const RELEASE_END = 62.4
const START_END = 65.6
const FILM_END = 70.6

// A10: `ref-server` typed in a terminal (RB2), then the three windows of the standalone take (M2)
// close one after the other. The pill counts from the server start with no sessions; once the
// last window is closed it skips (time-skip chip) to the take's uptime and session count.
const PILL_DELAY_SECONDS = 0.6
// RB2 from just before `ref-server` is typed (cast marker "typed").
const SERVER_TYPED = { mark: 'typed', offset: -0.2 }
const CLOSE_STAGGER_SECONDS = 0.8
const FIRST_CLOSE_SECONDS = 0.6
const WINDOW_COUNT = 3
// Seconds the standalone server had been up when the windows were photographed, and the
// sessions it had then: main and four workers (printed by capture/standalone.ts).
const UPTIME_AT_CLOSE_SECONDS = 35
const SESSIONS_AT_CLOSE = { en: '5 sessions running', zh: '5 个会话在运行' }
const PILL_FROM = CREW_END + PILL_DELAY_SECONDS
const ALL_CLOSED_AT = SERVER_START_END + FIRST_CLOSE_SECONDS + (WINDOW_COUNT - 1) * CLOSE_STAGGER_SECONDS + CLOSE_SECONDS
const PILL_SKIP_SECONDS = UPTIME_AT_CLOSE_SECONDS - (ALL_CLOSED_AT - PILL_FROM)
const SKIP_CHIP_AT = { x: 1660, y: 100 }

// A hard cut: a fade would show the crew windows over the terminal.
const serverStartCut: Cut = drawnCut(
  CREW_END,
  SERVER_START_END,
  (shot, seconds) => {
    shot.add(castWindow({ ...SERVER_CAST, x: 260, y: 140, castFrom: SERVER_TYPED, start: 0, from: 0, to: seconds }))
  },
  'cut',
)

const standaloneCut: Cut = {
  from: SERVER_START_END,
  to: STANDALONE_END,
  build: (seconds) =>
    threeWindowsShot(
      seconds,
      [
        { screens: [{ name: 'tui-standalone-clean' }], title: 'ref', look: { at: 0, x: 0.5, y: 0.5, zoom: 1 } },
        { screens: [{ name: 'gui-standalone' }], title: 'ref-gui', look: { at: 0, x: 0.6, y: 0.4, zoom: 1.3 } },
        // Below the prompt, which starts with the mock's script name.
        { screens: [{ name: 'web-standalone' }], title: 'ref-web', look: { at: 0, x: 0.55, y: 0.55, zoom: 1.3 } },
      ].map((window, index) => ({ ...window, closeAt: FIRST_CLOSE_SECONDS + index * CLOSE_STAGGER_SECONDS })),
      false,
    ),
}

// A11: three closeups, one feature each. f1 is the row "Retrying in 8s (attempt 3)" of the
// outage take (row 17), cut before the rest of the error and above the rows with the mock's
// local address. The text runs from column 3 (the bar) to 33 (the colon).
const RETRY_CLOSEUP = { scale: 2.5, x: 18.6 / 150, y: 17.5 / 40, rows: 1.3, cols: 31.4 }
const RETRY_CLOSEUP_AT = { x: 520, y: 480 }
// f2: the delete row with Cancel and its countdown, and the bash row above it.
const DELAY_ROW_LOOK = { at: 0, x: 0.585, y: 0.278, zoom: 2 }
// f3: the model dialog of the real-backend take (cast marker "models": ctrl+x m is pressed). The
// dialog is open for 3.0 s (capture/real.ts); the callouts end as it closes, before the cut's tail.
const MODEL_DIALOG = { mark: 'models', offset: -0.3 }
const MODEL_MARKS_FROM = 0.7
const MODEL_MARKS_TO = 3.2
// f2: Cancel is clicked this long into the cut, after about 2.3 s of countdown.
const DELAY_CANCEL_AT = 2.6
const featureCuts: Cut[] = [
  drawnCut(STANDALONE_END, STANDALONE_END + FEATURE_CUT_SECONDS, (shot, seconds) => {
    shot.add(
      castWindow({
        ...OUTAGE_CAST,
        ...WIDE_TUI,
        ...RETRY_CLOSEUP_AT,
        closeup: RETRY_CLOSEUP,
        castFrom: { mark: 'attempt3', offset: -0.3 },
        start: 0,
        from: -1,
        to: seconds + 1,
      }),
    )
  }),
  {
    from: STANDALONE_END + FEATURE_CUT_SECONDS,
    to: STANDALONE_END + 2 * FEATURE_CUT_SECONDS,
    build: (seconds) =>
      beatShot({ seconds, ...clickedClip('gui-delay', 'cancel', DELAY_CANCEL_AT), looks: [DELAY_ROW_LOOK], windowFrom: -1 }),
  },
  drawnCut(STANDALONE_END + 2 * FEATURE_CUT_SECONDS, STANDALONE_END + 3 * FEATURE_CUT_SECONDS, (shot, seconds) => {
    shot.add(castWindow({ ...FIXTURE_CAST, castFrom: MODEL_DIALOG, marks: opencodeModelMarks(MODEL_MARKS_FROM, MODEL_MARKS_TO), start: 0, from: -1, to: seconds }))
  }, 'quick'),
]

// A12: the release list, then the two commands, then a hard cut to ref starting (a fade would
// show the typed commands over the splash).
const START_TYPING_SECONDS = 1.8
// The real-backend take from just before ref is ready (cast marker "ready"): the end of its start.
const REF_STARTING = { mark: 'ready', offset: -0.1 }
const COMMANDS_END = RELEASE_END + START_TYPING_SECONDS
const startCuts: Cut[] = [
  drawnCut(RELEASE_END, COMMANDS_END, (shot, seconds) =>
    shot.add(
      fakeTerminal({
        rect: HOOK_RECT,
        lines: [
          { at: 0.2, typed: true, prompt: '$ ', text: 'export ANTHROPIC_API_KEY=...', charsPerSecond: 28 },
          { at: 1.4, typed: true, prompt: '$ ', text: 'ref', charsPerSecond: 10 },
        ],
        from: 0,
        to: seconds + 1,
      }),
    ),
  ),
  drawnCut(
    COMMANDS_END,
    START_END,
    (shot, seconds) => shot.add(castWindow({ ...FIXTURE_CAST, castFrom: REF_STARTING, start: 0, from: -1, to: seconds + 1 })),
    'cut',
  ),
]

export const scene = timelineFilm({
  id: 'intro',
  cuts: [
    waitingTabsCut(0, HERO_FILM_START),
    // Hard cut at the Enter: a fade would show the hook terminal through the title card.
    // The report frame holds until its caption ends at 21.9 (the report is at film 19.4).
    heroCut(HERO_FILM_START, A6_END, 0, { holdReport: 2.6, enter: 'cut' }),
    heroCut(A6_END, HERO_END, A7_CLIP_FROM, { enter: 'cut' }),
    drawnCut(HERO_END, CARD_END, (shot, seconds) => shot.add(compareCard({ ...COMPARE, from: 0, to: seconds + 1 }))),
    crewWindowsCut(CARD_END, CREW_END),
    serverStartCut,
    standaloneCut,
    ...featureCuts,
    drawnCut(STANDALONE_END + 3 * FEATURE_CUT_SECONDS, RELEASE_END, (shot, seconds) => shot.add(releaseCard({ ...RELEASE, from: 0, to: seconds + 1 }))),
    ...startCuts,
    endCut(START_END, FILM_END, { platforms: true }, 'quick'),
  ],
  captions: cues([
    ...HERO_CAPTIONS,
    [26.1, 29.9, COPY.finished],
    [31.0, 33.9, { en: 'All worker sessions stream in one view.', zh: '所有工作会话在分屏里同时输出。' }],
    [34.7, 38.0, { en: "Answer a worker's permission request in its tile.", zh: '授权请求在它自己的分块里点允许。' }],
    [38.6, 42.2, COPY.sameSessions],
    [43.6, 46.1, { en: 'Start the backend yourself.', zh: '单独启动后端。' }],
    [46.2, 49.2, { en: 'Close every window. The sessions keep running.', zh: '窗口全关，会话照跑。' }],
    [49.6, 52.6, { en: 'ref retries when the API fails.', zh: '接口出错，ref 会自动重试。' }],
    [53.0, 56.0, { en: 'Set a delay: Cancel stops the call.', zh: '设几秒延迟，点取消就能停掉调用。' }],
    [56.4, 59.4, { en: 'ref reads your opencode.json.', zh: 'ref 直接读你的 opencode.json。' }],
    [59.7, 62.3, { en: 'Download from GitHub Releases.', zh: '到 GitHub Releases 下载。' }],
    [62.5, 65.5, { en: 'Set an API key and run ref.', zh: '设好 API key，运行 ref。' }],
    [65.8, 69.0, { en: 'ref: an open-source coding agent.', zh: 'ref：开源的编程智能体。' }],
  ]),
  overlay: (shot) => {
    addBrandTitle(shot, HERO_END)
    shot.add(
      serverPill({
        timerStart: 0,
        jumps: [{ at: ALL_CLOSED_AT, seconds: PILL_SKIP_SECONDS }],
        sessions: { text: SESSIONS_AT_CLOSE, from: ALL_CLOSED_AT },
        from: PILL_FROM,
        to: STANDALONE_END,
      }),
    )
    shot.add(timeSkipChip({ text: `+${Math.round(PILL_SKIP_SECONDS)} s`, ...SKIP_CHIP_AT, from: ALL_CLOSED_AT, to: STANDALONE_END }))
    const chips = [FEATURES.retries, FEATURES.delay, FEATURES.opencode]
    chips.forEach((chip, index) => {
      const from = STANDALONE_END + index * FEATURE_CUT_SECONDS
      shot.add(featureTag({ ...chip, from: from + 0.1, to: from + FEATURE_CUT_SECONDS }))
    })
  },
})
