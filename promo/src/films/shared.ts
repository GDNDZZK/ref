// Cuts that Film A, the trailer and the feature films share.
import { brandTitle } from '../components/brandTitle.ts'
import { clip } from '../engine/clips.ts'
import type { ShotBuilder } from '../engine/shot.ts'
import type { Pointer } from '../shots/beat.ts'
import { threeWindowsShot } from '../shots/threeWindows.ts'
import { BRAND_TITLE_SECONDS, COPY, HERO_FILM_START } from './copy.ts'
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

// A2, B2, C1 shot 2: the lockup and the tagline over the composer, then the corner mark
// until `markUntil`.
export function addBrandTitle(shot: ShotBuilder, markUntil: number) {
  shot.add(
    brandTitle({
      tagline: COPY.tagline,
      from: HERO_FILM_START,
      shrinkAt: HERO_FILM_START + BRAND_TITLE_SECONDS,
      to: markUntil,
    }),
  )
}

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
