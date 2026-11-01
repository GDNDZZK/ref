// Cuts that Film A, the trailer and the feature films share.
import { brandTitle } from '../components/brandTitle.ts'
import { clip } from '../engine/clips.ts'
import type { ShotBuilder } from '../engine/shot.ts'
import type { Pointer } from '../shots/beat.ts'
import { threeWindowsShot } from '../shots/threeWindows.ts'
import { BRAND_TITLE_SECONDS, HERO_FILM_START } from './copy.ts'
import type { Cut } from './timeline.ts'

// A9, B6: the crew of the hero script in the terminal, the desktop app and the browser at the
// same moment (capture crew.ts).
export function crewWindowsCut(from: number, to: number): Cut {
  return {
    from,
    to,
    build: (seconds) =>
      threeWindowsShot(seconds, [
        { screens: [{ name: 'tui-crew-clean' }], title: 'ref', look: { at: 0, x: 0.5, y: 0.5, zoom: 1 } },
        { screens: [{ name: 'gui-crew' }], title: 'ref-gui', look: { at: 0, x: 0.6, y: 0.4, zoom: 1.3 } },
        { screens: [{ name: 'web-crew' }], title: 'ref-web', look: { at: 0, x: 0.5, y: 0.3, zoom: 1.02 } },
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
