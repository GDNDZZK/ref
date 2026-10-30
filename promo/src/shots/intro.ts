import { backdrop } from '../components/backdrop.ts'
import { titleCard } from '../components/titleCard.ts'
import { defineShot } from '../engine/shot.ts'
import type { Localized } from '../engine/i18n.ts'

const INTRO_SECONDS = 2.8

// Logo, name and one line of copy.
export function introShot(tagline: string | Localized) {
  return defineShot(INTRO_SECONDS, (shot) => {
    shot.add(backdrop())
    shot.add(titleCard({ title: 'ref', tagline, logo: true, from: 0.2, to: INTRO_SECONDS }))
  })
}
