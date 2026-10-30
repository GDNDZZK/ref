import { backdrop } from '../components/backdrop.ts'
import { titleCard } from '../components/titleCard.ts'
import { defineShot } from '../engine/shot.ts'
import type { Localized } from '../engine/i18n.ts'

const OUTRO_SECONDS = 2.4
const REPOSITORY = 'github.com/GDNDZZK/ref'

// The closing card with the repository address.
export function outroShot(tagline: string | Localized = REPOSITORY) {
  return defineShot(OUTRO_SECONDS, (shot) => {
    shot.add(backdrop())
    shot.add(titleCard({ title: 'ref', tagline, logo: true, from: 0.1, to: OUTRO_SECONDS + 1 }))
  })
}
