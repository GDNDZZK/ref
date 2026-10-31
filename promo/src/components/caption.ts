import { fadeWindow } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface CaptionCue {
  from: number
  to: number
  text: string | Localized
}

const FADE_SECONDS = 0.25
const RISE_PX = 16

// Subtitle pill at the bottom of the frame. One cue shows at a time.
export function caption(cues: readonly CaptionCue[]): Part {
  const el = element('div', 'part caption')
  const text = element('span', 'caption-text', el)

  return {
    el,
    render(time) {
      const cue = cues.find((candidate) => time >= candidate.from && time < candidate.to)
      el.style.display = cue ? '' : 'none'
      if (!cue) return

      text.textContent = tr(cue.text)
      const visibility = fadeWindow(time, cue.from, cue.to, FADE_SECONDS)
      el.style.opacity = String(visibility)
      el.style.transform = `translateY(${(1 - visibility) * RISE_PX}px)`
    },
  }
}
