import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface FeatureTagOptions {
  // Inline SVG markup of an icon from assets/icons.
  icon: string
  name: string | Localized
  from: number
  to: number
}

const ENTER_SECONDS = 0.5
const EXIT_SECONDS = 0.25
const RISE_PX = 24

// Small pill with an icon and the feature's name, top left above the window.
export function featureTag(options: FeatureTagOptions): Part {
  const el = element('div', 'part feature-tag')
  const icon = element('span', 'feature-tag-icon', el)
  icon.innerHTML = options.icon
  const label = element('span', 'feature-tag-name', el)
  label.textContent = tr(options.name)

  return {
    el,
    render(time) {
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
      el.style.transform = `translateY(${(1 - entering) * RISE_PX}px)`
    },
  }
}
