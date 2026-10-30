// Per-frame exponential chase (Motion.qml `chase` / `arrived`, ChatView.scrollStep).
//
// Each frame covers 1 - e^(-frameTime / followLag) of the remaining distance, so a target that
// keeps moving never restarts the motion and the speed does not depend on the frame rate.

import { motion } from '../theme/generated/motion.ts'

const MS_PER_SECOND = 1000

export function arrived(current: number, target: number): boolean {
  return Math.abs(target - current) < motion.chaseSnap
}

// One frame of a chase; `frameSeconds` is the time since the previous frame.
export function chase(current: number, target: number, frameSeconds: number): number {
  if (arrived(current, target)) return target
  const covered = 1 - Math.exp((-frameSeconds * MS_PER_SECOND) / motion.followLag)
  return current + (target - current) * covered
}

// One frame of a scroll chase toward `target`, in whole pixels and at least one per frame
// (ChatView.scrollStep). The browser rounds scroll offsets to device pixels, so a sub-pixel
// step that is rounded away would stall the chase a few pixels short of the target.
export function scrollStep(position: number, target: number, frameSeconds: number): number {
  const from = Math.round(position)
  const next = Math.round(chase(position, target, frameSeconds))
  const step = next !== from ? next : from + Math.sign(target - from)
  const overshoots = (step - target) * (from - target) < 0 // ends up past a fractional target
  return overshoots ? target : step
}

// Within a pixel of the target counts as there: scrolling moves in whole pixels.
export function scrollArrived(position: number, target: number): boolean {
  return Math.abs(target - position) < 1
}
