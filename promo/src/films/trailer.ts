// Film B, the trailer (shooting script section 4): cuts of Film A from the same recording.
import { heroCut } from '../shots/hero.ts'
import { waitingTabsCut } from '../shots/terminalHook.ts'
import { COPY, HERO_CAPTIONS, HERO_FILM_START } from './copy.ts'
import { addBrandTitle, crewWindowsCut } from './shared.ts'
import { cues, endCut, timelineFilm } from './timeline.ts'

// B2-B4 run the recording from t = 0 to 13.6; B5 jumps to t = 15.0.
const FIRST_SLICE_END = 17.0
const SECOND_SLICE_CLIP_FROM = 15.0
const SECOND_SLICE_END = 23.0
const WINDOWS_END = 27.0
const FILM_END = 31.0

export const scene = timelineFilm({
  id: 'trailer',
  cuts: [
    waitingTabsCut(0, HERO_FILM_START),
    heroCut(HERO_FILM_START, FIRST_SLICE_END, 0),
    heroCut(FIRST_SLICE_END, SECOND_SLICE_END, SECOND_SLICE_CLIP_FROM, 'cut'),
    crewWindowsCut(SECOND_SLICE_END, WINDOWS_END),
    endCut(WINDOWS_END, FILM_END, { platforms: true }),
  ],
  captions: cues([
    ...HERO_CAPTIONS,
    [23.2, 26.8, COPY.sameSessions],
    [27.2, 30.4, { en: 'Open source on GitHub.', zh: '开源，代码在 GitHub。' }],
  ]),
  overlay: (shot) => addBrandTitle(shot, SECOND_SLICE_END),
})
