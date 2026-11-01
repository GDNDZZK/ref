// C1 "The main agent starts worker sessions" (shooting script section 5): Film A's A1-A7.
import { heroCut } from '../shots/hero.ts'
import { waitingTabsCut } from '../shots/terminalHook.ts'
import { BRAND_TITLE_SECONDS, COPY, HERO_CAPTIONS, HERO_FILM_START } from './copy.ts'
import { FEATURES } from './features.ts'
import { addBrandTitle } from './shared.ts'
import { cues, endCut, timelineFilm } from './timeline.ts'

const A6_END = 24.4
const A7_CLIP_FROM = 21.0
const HERO_END = 29.9
const FILM_END = 32.4

export const scene = timelineFilm({
  id: 'crew',
  cuts: [
    waitingTabsCut(0, HERO_FILM_START),
    // Hard cut at the Enter: a fade would show the hook terminal through the title card.
    // The report frame holds until its caption ends at 21.9 (the report is at film 19.4).
    heroCut(HERO_FILM_START, A6_END, 0, { holdReport: 2.6, enter: 'cut' }),
    heroCut(A6_END, HERO_END, A7_CLIP_FROM, { enter: 'cut' }),
    endCut(HERO_END, FILM_END),
  ],
  captions: cues([...HERO_CAPTIONS, [27.4, 29.9, COPY.finished]]),
  chip: { ...FEATURES.crew, from: HERO_FILM_START + BRAND_TITLE_SECONDS + 0.4, to: HERO_END },
  overlay: (shot) => addBrandTitle(shot, HERO_END),
})
