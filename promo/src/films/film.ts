import { backdrop } from '../components/backdrop.ts'
import { caption } from '../components/caption.ts'
import { featureTag } from '../components/featureTag.ts'
import { textCard, type TextCardOptions } from '../components/textCard.ts'
import type { SceneDefinition, Step } from '../engine/scene.ts'
import { defineShot } from '../engine/shot.ts'
import { beatShot, captionCues, type BeatOptions } from '../shots/beat.ts'
import { introShot } from '../shots/intro.ts'
import { outroShot } from '../shots/outro.ts'
import type { Feature } from './features.ts'

// Shots overlap by this long; every shot but the last carries the overlap as extra length.
export const TRANSITION_SECONDS = 0.5
const TITLE_SECONDS = 2.5

// A shot that shows text cards (a config file, a message) instead of a captured window.
export type CardBeatOptions = Pick<BeatOptions, 'seconds' | 'caption' | 'tag'> & {
  cards: readonly Omit<TextCardOptions, 'from' | 'to'>[]
}

export type Beat = ({ kind: 'window' } & BeatOptions) | ({ kind: 'cards' } & CardBeatOptions)

function cardShot(options: CardBeatOptions) {
  return defineShot(options.seconds, (shot) => {
    shot.add(backdrop())
    for (const card of options.cards) shot.add(textCard({ ...card, from: 0.2, to: options.seconds }))
    if (options.tag) shot.add(featureTag({ ...options.tag, from: 0.1, to: options.seconds }))
    if (options.caption) shot.add(caption(captionCues(options.caption, options.seconds)))
  })
}

// `seconds` in a beat is what the viewer sees; the overlap is added here.
export function stepFor(beat: Beat): Step {
  const seconds = beat.seconds + TRANSITION_SECONDS
  const shot = beat.kind === 'window' ? beatShot({ ...beat, seconds }) : cardShot({ ...beat, seconds })
  return { shot, enter: { kind: 'fade', duration: TRANSITION_SECONDS } }
}

export function windowBeat(options: BeatOptions): Beat {
  return { kind: 'window', ...options }
}

export function cardsBeat(options: CardBeatOptions): Beat {
  return { kind: 'cards', ...options }
}

// A feature film: title card, the beats, the closing card.
export function filmScene(feature: Feature, beats: readonly Beat[]): SceneDefinition {
  return {
    id: feature.id,
    steps: [
      { shot: titleShot(feature) },
      ...beats.map(stepFor),
      { shot: outroShot(), enter: { kind: 'rise', duration: TRANSITION_SECONDS } },
    ],
  }
}

function titleShot(feature: Feature) {
  return introShot(feature.tagline, { title: feature.name, logo: false, seconds: TITLE_SECONDS + TRANSITION_SECONDS })
}

// The feature's icon and name for the tag above the window.
export function tagOf(feature: Feature) {
  return { icon: feature.icon, name: feature.name }
}
