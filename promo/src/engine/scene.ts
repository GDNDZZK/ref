import { span } from './anim.ts'
import type { Shot } from './shot.ts'

export type TransitionKind = 'fade' | 'rise'

// How a shot comes in over the one before it. The shots overlap by `duration`, so the
// previous shot's own duration must include that tail.
export interface Transition {
  kind: TransitionKind
  duration: number
}

export interface Step {
  shot: Shot
  // Without a transition the shot cuts in at the end of the previous one.
  enter?: Transition
}

export interface SceneDefinition {
  id: string
  steps: readonly Step[]
}

export interface PlacedStep {
  step: Step
  start: number
  end: number
}

export function placeSteps(steps: readonly Step[]): PlacedStep[] {
  const placed: PlacedStep[] = []
  let previousEnd = 0
  for (const step of steps) {
    const overlap = step.enter?.duration ?? 0
    const start = Math.max(0, previousEnd - overlap)
    const end = start + step.shot.duration
    placed.push({ step, start, end })
    previousEnd = end
  }
  return placed
}

const RISE_DISTANCE_PX = 48 // motion.enterDrop in the product
const ENTER_EASING = 'inOutSine'

// Opacity and offset of a shot `sinceStart` seconds after it began, for its entrance.
export function entrance(transition: Transition | undefined, sinceStart: number) {
  if (!transition) return { opacity: 1, offsetY: 0 }
  const progress = span(sinceStart, 0, transition.duration, ENTER_EASING)
  const offsetY = transition.kind === 'rise' ? (1 - progress) * RISE_DISTANCE_PX : 0
  return { opacity: progress, offsetY }
}
