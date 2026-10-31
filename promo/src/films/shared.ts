// Cuts that Film A, the trailer and the feature films share.
import { brandTitle } from '../components/brandTitle.ts'
import type { ShotBuilder } from '../engine/shot.ts'
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
