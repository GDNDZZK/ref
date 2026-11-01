// The hero: one continuous recording of the `orchestrate` mock script in ref-gui (capture M1,
// capture/orchestra.ts "orchestra-hero"). Every time here is mock time `t`, seconds after Enter
// on the pre-typed prompt; films place slices of it. Picture coordinates are fractions of the
// 1280x1120 window and are tuned against the recording.
import { notificationCard } from '../components/notificationCard.ts'
import type { Cut } from '../films/timeline.ts'
import { findClip } from '../engine/clips.ts'
import type { Localized } from '../engine/i18n.ts'
import { beatShot, type Look } from './beat.ts'

export const HERO_CLIP = 'gui-hero'
export const HERO_SIZE = { width: 1280, height: 1120 }
// The window is wider than the tall picture would fit, so the camera always crops it and the
// text stays readable; zoom 1 shows the whole width.
const HERO_RECT = { x: 180, y: 70, width: 1560, height: 880 }
// The recording ends with the summary at t = 25.2; films hold its last frame after that.
export const HERO_END_T = 25.2

// The scripted clicks (capture/orchestra.ts "hero"): the tester tile at 7.0, Allow at 13.0. The
// recording's manifest has where and when they really happened; these stand in until it exists.
const SCRIPTED_CLICKS = [
  { at: 7.0, x: 0.32, y: 0.555 },
  { at: 13.0, x: 0.74, y: 0.47 },
]
const POINTER_APPROACH_SECONDS = 1.1
const POINTER_REST = { x: 0.9, y: 0.75 }
const FIXER_NOTICE_T = 11.5
const FINISHED_NOTICE_T = 25.0
const NOTICE_SECONDS = 3.2

// Where things are in the split view (main and scout on top, fixer and tester in the middle,
// docs and later reviewer at the bottom).
const COMPOSER = { x: 0.6, y: 0.9 }
const GRID = { x: 0.6, y: 0.5 }
// While main starts the workers: the sidebar (its Todo and the Running rows that appear) and
// main's tile with the create_session calls. The new tiles open on a raw "created by main"
// message, which stays out of view.
const MAIN_COLUMN = { x: 0.3, y: 0.25, zoom: 1.68 }
// The top two rows (main, scout, fixer, tester) before the click on the tester; the docs tile
// below opens on its raw "created by main" message and stays out of view.
const GRID_WATCH = { x: 0.6, y: 0.36, zoom: 1 }
const TESTER_TILE = { x: 0.72, y: 0.52 }
// The fixer's permission card: its edit header down to the three buttons.
const FIXER_CARD = { x: 0.41, y: 0.505, zoom: 2.5 }
// Main's tile from its header to the status line, while the reviewer reports and the summary
// streams in (A7).
const MAIN_SUMMARY = { x: 0.41, y: 0.215, zoom: 2 }
// Main's status-line to-do item once all five are done.
const MAIN_TODO_DONE = { x: 0.447, y: 0.33, width: 0.033, height: 0.021 }
const TODO_DONE_MARK_T = 24.8

// The tester's report shows in main's tile from 15.6 until the fixer's report pushes it up at
// 16.5, too short to read; films that show it hold the frame of the recording's "report" mark
// (about 16.0), with a spotlight on the line, until their caption about it has ended.
const SCRIPTED_REPORT_AT = 16.0
// The report line on the held frame, measured on both takes; re-measure after a retake (the
// message above it wraps differently from take to take).
const REPORT_LINE = { x: 0.246, y: 0.084, width: 0.322, height: 0.028 }
// The camera leaves the report here, just after the report mark (about 16.0), so a held report
// frame keeps the look. The spotlight stays on while the camera pulls out past the raw
// "[Reply with send_message …]" lines around the report, and fades out over SPOTLIGHT_FADE as
// the grid comes into view (the pull-out takes 0.8 s).
const REPORT_LOOK_END_T = 16.1
const SPOTLIGHT_AFTER_MOVE = 0.7
const SPOTLIGHT_FADE = 0.3
// Right after Allow the edit card shows the absolute mock path for a frame or two; the films
// show the frame before it instead.
const PATH_FRAMES = { from: 13.0, to: 13.3 }

// Camera in mock time; `report` is the centre of the tester's report line.
const looks = (report: { x: number; y: number }): Look[] => [
  { at: 0, ...COMPOSER, zoom: 1.5 },
  { at: 2.5, ...COMPOSER, zoom: 1.5 },
  { at: 3.3, ...MAIN_COLUMN },
  { at: 5.2, ...MAIN_COLUMN },
  { at: 5.9, ...GRID_WATCH },
  { at: 6.7, ...GRID_WATCH },
  { at: 7.3, ...TESTER_TILE, zoom: 1.6 },
  { at: 10.8, ...TESTER_TILE, zoom: 1.6 },
  { at: 11.4, ...FIXER_CARD },
  { at: 14.6, ...FIXER_CARD },
  { at: 15.3, ...report, zoom: 2.6 },
  { at: REPORT_LOOK_END_T, ...report, zoom: 2.6 },
  { at: 16.9, ...GRID, zoom: 1 },
  { at: 20.6, ...GRID, zoom: 1 },
  // A7 cuts in after this (A7_CLIP_FROM in intro.ts and crew.ts).
  { at: 21.0, ...MAIN_SUMMARY },
]

// The recording in mock time: where its Enter is, and its clicks.
function recording() {
  const recorded = findClip(HERO_CLIP)
  if (!recorded) return { enterAt: 0, clicks: SCRIPTED_CLICKS, reportAt: SCRIPTED_REPORT_AT }
  const enterAt = recorded.marks.enter
  const reportAt = recorded.marks.report
  if (enterAt === undefined || reportAt === undefined) throw new Error(`${HERO_CLIP} has no "enter" or "report" mark; capture it again`)
  return {
    enterAt,
    clicks: recorded.clicks.map((click) => ({ ...click, at: click.at - enterAt })),
    reportAt: reportAt - enterAt,
  }
}

// The pointer rests, moves to each click in POINTER_APPROACH_SECONDS, stays there until it
// moves on to the next, and goes back to rest after the last.
function pointerPath(clicks: readonly { at: number; x: number; y: number }[]) {
  const keys: { at: number; x: number; y: number }[] = []
  let position = POINTER_REST
  for (const click of clicks) {
    keys.push({ at: click.at - POINTER_APPROACH_SECONDS, ...position }, { at: click.at - 0.05, x: click.x, y: click.y })
    position = { x: click.x, y: click.y }
  }
  const last = clicks.at(-1)
  if (last) keys.push({ at: last.at + 0.6, ...position }, { at: last.at + 0.6 + POINTER_APPROACH_SECONDS, ...POINTER_REST })
  return keys
}

const NOTICES: readonly { at: number; title: string; body: Localized }[] = [
  { at: FIXER_NOTICE_T, title: 'fixer', body: { en: 'Waiting for permission to use edit', zh: '等待授权使用 edit' } },
  { at: FINISHED_NOTICE_T, title: 'main', body: { en: 'Finished', zh: '已完成' } },
]

export interface HeroCutOptions {
  enter?: Cut['enter']
  // Hold the frame with the tester's report this many seconds when the cut reaches it, before the
  // camera pulls out (the picture stays on it through the pull-out, see heroCut).
  holdReport?: number
}

// Film seconds [from, to) showing the recording from mock time `clipFrom` on.
export function heroCut(from: number, to: number, clipFrom: number, options: HeroCutOptions = {}): Cut {
  const { enterAt, clicks, reportAt } = recording()
  const hold = options.holdReport ? { at: reportAt, seconds: options.holdReport } : { at: Infinity, seconds: 0 }
  // Shot seconds of mock time `t` for the camera.
  const shift = (t: number) => t - clipFrom + (t > hold.at ? hold.seconds : 0)
  // The spotlight ends after the camera has pulled out of the report; the picture stays on the
  // report frame until then, so the spotlight keeps framing the report line, and plays on from
  // there, `freeze` seconds behind the recording.
  const spotlightEnd = shift(REPORT_LOOK_END_T) + SPOTLIGHT_AFTER_MOVE
  const freeze = options.holdReport ? spotlightEnd - shift(reportAt) : 0
  // Shot seconds of mock time `t` in the picture, and the other way round.
  const pictureShift = (t: number) => t - clipFrom + (t > hold.at ? freeze : 0)
  const mockTime = (time: number) => {
    const raw = clipFrom + time
    const held = raw < hold.at ? raw : Math.max(hold.at, raw - freeze)
    return held >= PATH_FRAMES.from && held < PATH_FRAMES.to ? PATH_FRAMES.from : held
  }
  return {
    from,
    to,
    enter: options.enter,
    build: (seconds) =>
      beatShot({
        seconds,
        screens: [{ clip: HERO_CLIP, time: (time) => enterAt + mockTime(time) }],
        pendingSize: HERO_SIZE,
        rect: HERO_RECT,
        title: 'ref',
        windowFrom: -1,
        looks: looks({ x: REPORT_LINE.x + REPORT_LINE.width / 2, y: REPORT_LINE.y + REPORT_LINE.height / 2 }).map((look) => ({ ...look, at: shift(look.at) })),
        marks: [
          ...(options.holdReport ? [{ ...REPORT_LINE, spotlight: true, from: shift(reportAt) - 0.1, to: spotlightEnd, exitSeconds: SPOTLIGHT_FADE }] : []),
          { ...MAIN_TODO_DONE, from: pictureShift(TODO_DONE_MARK_T), to: pictureShift(HERO_END_T) + 10 },
        ],
        pointer: {
          path: pointerPath(clicks).map((key) => ({ ...key, at: shift(key.at) })),
          clicks: clicks.map((click) => shift(click.at)),
        },
        extras: (shot) => {
          for (const notice of NOTICES) {
            const at = pictureShift(notice.at)
            if (at + NOTICE_SECONDS < 0 || at > seconds) continue
            shot.add(notificationCard({ title: notice.title, body: notice.body, from: at, to: at + NOTICE_SECONDS }))
          }
        },
      }),
  }
}
