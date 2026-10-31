import { backdrop } from '../components/backdrop.ts'
import { titleCard } from '../components/titleCard.ts'
import { defineShot } from '../engine/shot.ts'
import type { Localized } from '../engine/i18n.ts'

export const INTRO_SECONDS = 2.8

export interface IntroOptions {
  title?: string | Localized
  // The logo above the title; the feature films leave it out.
  logo?: boolean
  seconds?: number
}

// Name and one line of copy, with the logo for the trailer.
export function introShot(tagline: string | Localized, options: IntroOptions = {}) {
  const seconds = options.seconds ?? INTRO_SECONDS
  return defineShot(seconds, (shot) => {
    shot.add(backdrop())
    shot.add(titleCard({ title: options.title ?? 'ref', tagline, logo: options.logo ?? true, from: 0.2, to: seconds }))
  })
}
