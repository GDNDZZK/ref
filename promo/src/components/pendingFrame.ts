import { span } from '../engine/anim.ts'
import { notePending } from '../engine/pending.ts'
import { element, type Part } from '../engine/part.ts'
import type { AppFrame, Rect } from './appFrame.ts'
import { TITLE_BAR_HEIGHT_PX, createWindow } from './windowChrome.ts'

export interface PendingFrameOptions {
  // The capture names this window stands for, shown on it.
  names: readonly string[]
  title: string
  rect: Rect
  from: number
  to: number
}

const ENTER_SECONDS = 0.5
const EXIT_SECONDS = 0.35

// A window in place of a capture that does not exist yet, labelled with the capture's name.
// It has the AppFrame interface, so highlights and the pointer can be placed on it.
export function pendingFrame(options: PendingFrameOptions): AppFrame {
  for (const name of options.names) notePending(name)
  const { frame, body } = createWindow(options.title)
  const { rect } = options
  Object.assign(frame.style, { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.width}px`, height: `${rect.height}px` })
  const world = element('div', 'app-world pending-world', body)
  Object.assign(world.style, { width: '100%', height: '100%' })
  const label = element('div', 'pending-label', world)
  label.textContent = `PENDING CAPTURE\n${options.names.join('\n')}`
  const attached: Part[] = []

  return {
    el: frame,
    attach(part) {
      world.append(part.el)
      attached.push(part)
    },
    stagePoint(normalizedX, normalizedY) {
      return {
        x: rect.x + normalizedX * rect.width,
        y: rect.y + TITLE_BAR_HEIGHT_PX + normalizedY * (rect.height - TITLE_BAR_HEIGHT_PX),
      }
    },
    async render(time) {
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      frame.style.opacity = String(entering * (1 - leaving))
      await Promise.all(attached.map((part) => part.render(time)))
    },
  }
}
