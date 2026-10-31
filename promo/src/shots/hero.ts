// The hero: one continuous recording of the `orchestrate` mock script in ref-gui (capture M1,
// capture/orchestra.ts "orchestra-hero"). Every time here is mock time `t`, seconds after Enter
// on the pre-typed prompt; films place slices of it. Picture coordinates are fractions of the
// 1280x1120 window and are tuned against the recording.
import { notificationCard } from '../components/notificationCard.ts'
import type { Cut } from '../films/timeline.ts'
import type { Localized } from '../engine/i18n.ts'
import { beatShot, type Look } from './beat.ts'

export const HERO_CLIP = 'gui-hero'
export const HERO_SIZE = { width: 1280, height: 1120 }
// The recording ends with the summary at t = 25.2; films hold its last frame after that.
export const HERO_END_T = 25.2

// Mock times of the scripted input (capture/orchestra.ts uses the same numbers).
export const TESTER_CLICK_T = 7.0
export const ALLOW_CLICK_T = 13.0
const FIXER_NOTICE_T = 11.5
const FINISHED_NOTICE_T = 25.0
const NOTICE_SECONDS = 3.2

const COMPOSER = { x: 0.6, y: 0.9 }
const GRID = { x: 0.6, y: 0.45 }
const TESTER_TILE = { x: 0.47, y: 0.62 }
const FIXER_TILE = { x: 0.78, y: 0.3 }
const MAIN_TILE = { x: 0.47, y: 0.3 }
const TESTER_STATUS = { x: 0.32, y: 0.555 }
const FIXER_ALLOW = { x: 0.74, y: 0.47 }

const LOOKS: readonly Look[] = [
  { at: 0, ...COMPOSER, zoom: 1.7 },
  { at: 2.4, ...COMPOSER, zoom: 1.7 },
  { at: 3.2, ...GRID, zoom: 1.2 },
  { at: 6.6, ...GRID, zoom: 1.2 },
  { at: 7.4, ...TESTER_TILE, zoom: 1.5 },
  { at: 10.8, ...TESTER_TILE, zoom: 1.5 },
  { at: 11.6, ...FIXER_TILE, zoom: 1.5 },
  { at: 14.4, ...FIXER_TILE, zoom: 1.5 },
  { at: 15.2, ...TESTER_TILE, zoom: 1.6 },
  { at: 20.6, ...TESTER_TILE, zoom: 1.6 },
  { at: 21.4, ...MAIN_TILE, zoom: 1.4 },
]

const POINTER_PATH = [
  { at: 5.6, x: 0.55, y: 0.7 },
  { at: 6.9, ...TESTER_STATUS },
  { at: 11.8, ...TESTER_STATUS },
  { at: 12.9, ...FIXER_ALLOW },
  { at: 14.5, ...FIXER_ALLOW },
  { at: 15.5, x: 0.9, y: 0.75 },
]

const NOTICES: readonly { at: number; title: string; body: Localized }[] = [
  { at: FIXER_NOTICE_T, title: 'fixer', body: { en: 'Waiting for permission to use edit', zh: '等待授权使用 edit' } },
  { at: FINISHED_NOTICE_T, title: 'main', body: { en: 'Finished', zh: '已完成' } },
]

// Film seconds [from, to) showing the recording from mock time `clipFrom` on.
export function heroCut(from: number, to: number, clipFrom: number, enter?: Cut['enter']): Cut {
  const shift = (t: number) => t - clipFrom
  return {
    from,
    to,
    enter,
    build: (seconds) =>
      beatShot({
        seconds,
        screens: [{ clip: HERO_CLIP, at: -clipFrom }],
        pendingSize: HERO_SIZE,
        title: 'ref',
        windowFrom: -1,
        looks: LOOKS.map((look) => ({ ...look, at: shift(look.at) })),
        pointer: {
          path: POINTER_PATH.map((key) => ({ ...key, at: shift(key.at) })),
          clicks: [TESTER_CLICK_T, ALLOW_CLICK_T].map(shift),
        },
        extras: (shot) => {
          for (const notice of NOTICES) {
            const at = shift(notice.at)
            if (at + NOTICE_SECONDS < 0 || at > seconds) continue
            shot.add(notificationCard({ title: notice.title, body: notice.body, from: at, to: at + NOTICE_SECONDS }))
          }
        },
      }),
  }
}
