import lockupUrl from '../../../assets/app/ref-lockup-dark.svg'
import smallMarkUrl from '../../../assets/app/ref-small.svg'
import { lerp, span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface BrandTitleOptions {
  tagline: string | Localized
  from: number
  // The lockup shrinks into the small corner mark here; the mark stays until `to`.
  shrinkAt: number
  to: number
}

const ENTER_SECONDS = 0.7
const SHRINK_SECONDS = 0.5
const SCRIM_OPACITY = 0.78
// Where the lockup group ends up when it shrinks: the small mark's centre.
const CORNER = { x: 76, y: 60 }
const STAGE_CENTRE = { x: 960, y: 470 }
const SHRUNK_SCALE = 0.08
const MARK_ENTER_SECONDS = 0.3
const EXIT_SECONDS = 0.3

// N9: the ref lockup and the tagline as a title card over the running picture, which then
// shrinks to the small `r.` mark in the top-left corner.
export function brandTitle(options: BrandTitleOptions): Part {
  const el = element('div', 'part brand-title')
  const scrim = element('div', 'brand-scrim', el)
  const group = element('div', 'brand-group', el)
  const lockup = element('img', 'brand-lockup', group)
  lockup.src = lockupUrl
  element('p', 'brand-tagline', group).textContent = tr(options.tagline)
  const mark = element('img', 'brand-corner-mark', el)
  mark.src = smallMarkUrl

  return {
    el,
    render(time) {
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const shrinking = span(time, options.shrinkAt, SHRINK_SECONDS, 'inOutSine')
      scrim.style.opacity = String(SCRIM_OPACITY * entering * (1 - shrinking))
      const x = lerp(0, CORNER.x - STAGE_CENTRE.x, shrinking)
      const y = lerp((1 - entering) * 40, CORNER.y - STAGE_CENTRE.y, shrinking)
      group.style.transform = `translate(${x}px, ${y}px) scale(${lerp(1, SHRUNK_SCALE, shrinking)})`
      group.style.opacity = String(entering * (1 - span(time, options.shrinkAt + SHRINK_SECONDS * 0.6, SHRINK_SECONDS * 0.4)))
      const markShown = span(time, options.shrinkAt + SHRINK_SECONDS * 0.6, MARK_ENTER_SECONDS)
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      mark.style.opacity = String(markShown * (1 - leaving))
    },
  }
}
