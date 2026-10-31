// Films written against the shooting script's absolute times: cuts that follow each other,
// captions and the feature chip on a track over all cuts, so a caption can run across a cut.
import { backdrop } from '../components/backdrop.ts'
import { caption, type CaptionCue } from '../components/caption.ts'
import { endCard, type EndCardOptions } from '../components/endCard.ts'
import { featureTag } from '../components/featureTag.ts'
import type { SceneDefinition, Step } from '../engine/scene.ts'
import { defineShot, type Shot, type ShotBuilder } from '../engine/shot.ts'
import type { Localized } from '../engine/i18n.ts'

// A cross-fade between cuts; the outgoing cut runs this much longer under the incoming one.
export const FADE_SECONDS = 0.4

export interface Cut {
  // Film seconds. Each cut starts where the one before it ends.
  from: number
  to: number
  // `seconds` includes the overlap when the next cut fades in.
  build: (seconds: number) => Shot
  // How the cut comes in over the previous one: a cross-fade (default) or a hard cut, used
  // between slices of one recording.
  enter?: 'fade' | 'cut'
}

export interface Chip {
  icon: string
  name: Localized
  from: number
  to: number
}

export interface TimelineFilm {
  id: string
  cuts: readonly Cut[]
  captions: readonly CaptionCue[]
  chip?: Chip
  // More parts on the track over the cuts, in film time (the brand title, notifications).
  overlay?: (shot: ShotBuilder) => void
}

const CONTIGUOUS_TOLERANCE = 1e-6

export function timelineFilm(film: TimelineFilm): SceneDefinition {
  const steps: Step[] = film.cuts.map((cut, index) => {
    const previous = film.cuts[index - 1]
    if (previous && Math.abs(previous.to - cut.from) > CONTIGUOUS_TOLERANCE) {
      throw new Error(`${film.id}: cut ${index} starts at ${cut.from}, the one before ends at ${previous.to}`)
    }
    const next = film.cuts[index + 1]
    const overlap = next && next.enter !== 'cut' ? FADE_SECONDS : 0
    const fades = index > 0 && cut.enter !== 'cut'
    return { shot: cut.build(cut.to - cut.from + overlap), enter: fades ? { kind: 'fade', duration: FADE_SECONDS } : undefined }
  })
  const length = film.cuts.at(-1)?.to ?? 0
  const overlay = defineShot(length, (shot) => {
    if (film.chip) shot.add(featureTag({ icon: film.chip.icon, name: film.chip.name, from: film.chip.from, to: film.chip.to }))
    film.overlay?.(shot)
    shot.add(caption(film.captions))
  })
  return { id: film.id, steps, overlay }
}

// A cut that only draws parts over the backdrop.
export function drawnCut(from: number, to: number, draw: (shot: ShotBuilder, seconds: number) => void, enter?: Cut['enter']): Cut {
  return {
    from,
    to,
    enter,
    build: (seconds) =>
      defineShot(seconds, (shot) => {
        shot.add(backdrop())
        draw(shot, seconds)
      }),
  }
}

// N7 as a cut.
export function endCut(from: number, to: number, options: Omit<EndCardOptions, 'from' | 'to'> = {}): Cut {
  return drawnCut(from, to, (shot, seconds) => shot.add(endCard({ ...options, from: 0.1, to: seconds + 1 })))
}

// Caption cues from [from, to, text] rows, as the script's tables list them.
export function cues(rows: readonly (readonly [number, number, string | Localized])[]): CaptionCue[] {
  return rows.map(([from, to, text]) => ({ from, to, text }))
}
