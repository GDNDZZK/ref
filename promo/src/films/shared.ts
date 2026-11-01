// Cuts that Film A, the trailer and the feature films share.
import { brandTitle } from '../components/brandTitle.ts'
import { clip } from '../engine/clips.ts'
import type { ShotBuilder } from '../engine/shot.ts'
import type { Pointer } from '../shots/beat.ts'
import { threeWindowsShot } from '../shots/threeWindows.ts'
import { BRAND_TITLE_SECONDS, HERO_FILM_START } from './copy.ts'
import type { Cut } from './timeline.ts'

// A9, B6: the crew of the hero script in the terminal, the desktop app and the browser at the
// same moment, after main's summary (capture crew.ts). Each window starts whole, then zooms onto
// main's finished to-do list, the same detail in all three: the terminal's sidebar, the status
// line of main's tile and of the browser's conversation.
const CREW_ZOOM_AT = 1.6
const CREW_ZOOM_SECONDS = 0.7
const CREW_DETAIL = {
  tui: { look: { x: 0.86, y: 0.44, zoom: 2.4 }, mark: { x: 0.778, y: 0.322, width: 0.205, height: 0.16 } },
  gui: { look: { x: 0.37, y: 0.3, zoom: 2.4 }, mark: { x: 0.446, y: 0.326, width: 0.034, height: 0.023 } },
  web: { look: { x: 0.56, y: 0.84, zoom: 2 }, mark: { x: 0.702, y: 0.866, width: 0.033, height: 0.023 } },
}

function crewWindow(name: string, title: string, overview: { x: number; y: number; zoom: number }, detail: (typeof CREW_DETAIL)['tui'], seconds: number) {
  const zoomed = CREW_ZOOM_AT + CREW_ZOOM_SECONDS
  return {
    screens: [{ name }],
    title,
    look: [
      { at: 0, ...overview },
      { at: CREW_ZOOM_AT, ...overview },
      { at: zoomed, ...detail.look },
    ],
    marks: [{ ...detail.mark, from: zoomed, to: seconds + 1 }],
  }
}

export function crewWindowsCut(from: number, to: number): Cut {
  return {
    from,
    to,
    build: (seconds) =>
      threeWindowsShot(seconds, [
        crewWindow('tui-crew-clean', 'ref', { x: 0.5, y: 0.5, zoom: 1 }, CREW_DETAIL.tui, seconds),
        crewWindow('gui-crew', 'ref-gui', { x: 0.6, y: 0.4, zoom: 1.3 }, CREW_DETAIL.gui, seconds),
        crewWindow('web-crew', 'ref-web', { x: 0.5, y: 0.6, zoom: 1.02 }, CREW_DETAIL.web, seconds),
      ]),
  }
}

// A2, B2, C1 shot 2: the lockup over the composer (the tagline is the caption), then the
// corner mark until `markUntil`.
export function addBrandTitle(shot: ShotBuilder, markUntil: number) {
  shot.add(
    brandTitle({
      from: HERO_FILM_START,
      shrinkAt: HERO_FILM_START + BRAND_TITLE_SECONDS,
      to: markUntil,
    }),
  )
}

// A whole 150-column TUI take (the width that shows the session sidebar) across the stage.
export const WIDE_TUI = { fontSize: 19, x: 90, y: 70 }

// RB1 `ref` starting in ~/project with the fixture opencode.json (capture/real.ts).
export const FIXTURE_CAST = { name: 'tui-fixture-start', title: 'ref', ...WIDE_TUI }
// In its model dialog (cast marker "models" + 0.5 s on): the three models of opencode.json.
export function opencodeModelMarks(from: number, to: number) {
  return [
    { row: 11, col: 37, cols: 17 }, // Claude Sonnet 4.5
    { row: 14, col: 37, cols: 13 }, // DeepSeek Chat
    { row: 18, col: 37, cols: 11 }, // qwen3-coder
  ].map((cell) => ({ ...cell, from, to }))
}

// RB2 `ref-server` in a shell: two lines of output, so only the top rows, with room inside.
export const SERVER_CAST = { name: 'tui-server-start', title: '', fontSize: 24, topRows: 6, padding: 24 }

// The outage take (M5) as the films show it: the status line is cut off (it has the mock path).
export const OUTAGE_CAST = { name: 'tui-outage', title: 'ref', fontSize: 24, hideLastRows: 1, x: 168, y: 90 }

const POINTER_LEAD_SECONDS = 0.9
const POINTER_START_OFFSET = { x: 0.1, y: 0.14 }

// The clip `name` played so that its click after the mark `markName` falls at `clickAt`,
// with the pointer moving in to it.
export function clickedClip(name: string, markName: string, clickAt: number) {
  const recorded = clip(name)
  const markAt = recorded.marks[markName]
  const click = recorded.clicks.find((candidate) => markAt !== undefined && Math.abs(candidate.at - markAt) < 0.1)
  if (!click) throw new Error(`${name} has no click at the mark ${markName}`)
  const pointer: Pointer = {
    path: [
      { at: clickAt - POINTER_LEAD_SECONDS, x: click.x + POINTER_START_OFFSET.x, y: click.y + POINTER_START_OFFSET.y },
      { at: clickAt, x: click.x, y: click.y },
    ],
    clicks: [clickAt],
  }
  return { screens: [{ clip: name, at: clickAt - click.at }], pointer }
}
