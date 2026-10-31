// Film A "What is ref" (shooting script section 3). Times are film seconds from the script.
import { compareCard } from '../components/compareCard.ts'
import { fakeTerminal } from '../components/fakeTerminal.ts'
import { releaseCard } from '../components/releaseCard.ts'
import { serverPill } from '../components/serverPill.ts'
import { defineShot } from '../engine/shot.ts'
import { backdrop } from '../components/backdrop.ts'
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
import { addBrandTitle, crewWindowsCut } from './shared.ts'
import { cues, drawnCut, endCut, timelineFilm, type Cut } from './timeline.ts'

const HERO_END = 29.4
const CARD_END = 38.4
const CREW_END = 43.4
const SERVER_START_END = 45.4
const STANDALONE_END = 49.4
const FEATURE_CUT_SECONDS = 3.4
const RELEASE_END = 62.4
const START_END = 65.6
const FILM_END = 70.6

// A10: `ref-server` typed in a terminal (RB2), then the three windows of the standalone take (M2)
// close one second apart. The pill counts from the server start.
const PILL_DELAY_SECONDS = 0.6
const CLOSE_STAGGER_SECONDS = 1
const FIRST_CLOSE_SECONDS = 0.6
// Seconds the standalone server had been up when the windows closed, and the sessions it
// had then (from the take).
const UPTIME_AT_CLOSE_SECONDS = 62
const SESSIONS_AT_CLOSE = { en: '5 sessions running', zh: '5 个会话在运行' }

const serverStartCut: Cut = drawnCut(CREW_END, SERVER_START_END, (shot, seconds) => {
  shot.add(castWindow({ name: 'tui-server-start', title: '', x: 260, y: 140, fontSize: 24, start: 0, from: 0, to: seconds, pendingSize: { width: 1400, height: 600 } }))
})

const standaloneCut: Cut = {
  from: SERVER_START_END,
  to: STANDALONE_END,
  build: (seconds) =>
    threeWindowsShot(
      seconds,
      [
        { screens: [{ name: 'tui-standalone' }], title: 'ref', look: { at: 0, x: 0.5, y: 0.5, zoom: 1 } },
        { screens: [{ name: 'gui-standalone' }], title: 'ref-gui', look: { at: 0, x: 0.6, y: 0.4, zoom: 1.3 } },
        { screens: [{ name: 'web-standalone' }], title: 'ref-web', look: { at: 0, x: 0.5, y: 0.3, zoom: 1.02 } },
      ].map((window, index) => ({ ...window, closeAt: FIRST_CLOSE_SECONDS + index * CLOSE_STAGGER_SECONDS })),
      false,
    ),
}

// A11: three closeups, one feature each.
const OUTAGE_ATTEMPT_3_CAST_SECONDS = 14
const FIXTURE_MODEL_DIALOG_CAST_SECONDS = 6
const featureCuts: Cut[] = [
  drawnCut(STANDALONE_END, STANDALONE_END + FEATURE_CUT_SECONDS, (shot, seconds) => {
    shot.add(castWindow({ name: 'tui-outage', title: 'ref', x: 160, y: 120, fontSize: 30, start: -OUTAGE_ATTEMPT_3_CAST_SECONDS, from: -1, to: seconds }))
  }),
  {
    from: STANDALONE_END + FEATURE_CUT_SECONDS,
    to: STANDALONE_END + 2 * FEATURE_CUT_SECONDS,
    build: (seconds) =>
      beatShot({ seconds, screens: [{ clip: 'gui-delay' }], looks: [{ at: 0, x: 0.6, y: 0.6, zoom: 1.6 }], windowFrom: -1 }),
  },
  drawnCut(STANDALONE_END + 2 * FEATURE_CUT_SECONDS, STANDALONE_END + 3 * FEATURE_CUT_SECONDS, (shot, seconds) => {
    shot.add(castWindow({ name: 'tui-fixture-start', title: 'ref', x: 160, y: 120, fontSize: 28, start: -FIXTURE_MODEL_DIALOG_CAST_SECONDS, from: -1, to: seconds }))
  }),
]

// A12: the release list, then the two commands, then ref starting.
const START_TYPING_SECONDS = 1.8
const startCut: Cut = {
  from: RELEASE_END,
  to: START_END,
  build: (seconds) =>
    defineShot(seconds, (shot) => {
      shot.add(backdrop())
      shot.add(
        fakeTerminal({
          rect: HOOK_RECT,
          lines: [
            { at: 0.2, typed: true, prompt: '$ ', text: 'export ANTHROPIC_API_KEY=...', charsPerSecond: 28 },
            { at: 1.4, typed: true, prompt: '$ ', text: 'ref', charsPerSecond: 10 },
          ],
          from: 0,
          to: START_TYPING_SECONDS + 0.3,
        }),
      )
      shot.add(castWindow({ name: 'tui-start', title: 'ref', x: 260, y: 100, fontSize: 22, start: START_TYPING_SECONDS, from: START_TYPING_SECONDS, to: seconds + 1 }))
    }),
}

export const scene = timelineFilm({
  id: 'intro',
  cuts: [
    waitingTabsCut(0, HERO_FILM_START),
    heroCut(HERO_FILM_START, HERO_END, 0),
    drawnCut(HERO_END, CARD_END, (shot, seconds) => shot.add(compareCard({ ...COMPARE, from: 0, to: seconds + 1 }))),
    crewWindowsCut(CARD_END, CREW_END),
    serverStartCut,
    standaloneCut,
    ...featureCuts,
    drawnCut(STANDALONE_END + 3 * FEATURE_CUT_SECONDS, RELEASE_END, (shot, seconds) => shot.add(releaseCard({ ...RELEASE, from: 0, to: seconds + 1 }))),
    startCut,
    endCut(START_END, FILM_END, { platforms: true }),
  ],
  captions: cues([
    ...HERO_CAPTIONS,
    [26.5, 29.4, COPY.finished],
    [31.0, 33.9, { en: 'All worker sessions stream in one view.', zh: '所有工作会话在分屏里同时输出。' }],
    [34.7, 38.0, { en: "Answer a worker's permission request in its tile.", zh: '授权请求在它自己的分块里点允许。' }],
    [38.6, 42.2, COPY.sameSessions],
    [43.6, 46.1, { en: 'Start the backend yourself.', zh: '单独启动后端。' }],
    [46.2, 49.2, { en: 'Close every window. The sessions keep running.', zh: '窗口全关，会话照跑。' }],
    [49.6, 52.6, { en: 'ref retries when the API fails.', zh: '接口出错，ref 会自动重试。' }],
    [53.0, 56.0, { en: 'Set a delay: esc cancels the call.', zh: '设几秒延迟，按 esc 就能取消调用。' }],
    [56.4, 59.4, { en: 'ref reads your opencode.json.', zh: 'ref 直接读你的 opencode.json。' }],
    [59.7, 62.3, { en: 'Download from GitHub Releases.', zh: '到 GitHub Releases 下载。' }],
    [62.5, 65.5, { en: 'Set an API key and run ref.', zh: '设好 API key，运行 ref。' }],
    [65.8, 69.0, { en: 'ref: an open-source coding agent.', zh: 'ref：开源的编程智能体。' }],
  ]),
  overlay: (shot) => {
    addBrandTitle(shot, HERO_END)
    // The cut to the windows skips to just before the take's first close.
    const pillFrom = CREW_END + PILL_DELAY_SECONDS
    const uptimeAtCut = UPTIME_AT_CLOSE_SECONDS - FIRST_CLOSE_SECONDS
    shot.add(
      serverPill({
        timerStart: 0,
        jumps: [{ at: SERVER_START_END, seconds: uptimeAtCut - (SERVER_START_END - pillFrom) }],
        sessions: { text: SESSIONS_AT_CLOSE, from: SERVER_START_END },
        from: pillFrom,
        to: STANDALONE_END,
      }),
    )
    const chips = [FEATURES.retries, FEATURES.delay, FEATURES.opencode]
    chips.forEach((chip, index) => {
      const from = STANDALONE_END + index * FEATURE_CUT_SECONDS
      shot.add(featureTag({ ...chip, from: from + 0.1, to: from + FEATURE_CUT_SECONDS }))
    })
  },
})
