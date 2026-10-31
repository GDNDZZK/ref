import logoUrl from '../../../assets/app/ref.svg'
import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface TitleCardOptions {
  title: string | Localized
  tagline?: string | Localized
  logo?: boolean
  from: number
  to: number
}

const ENTER_SECONDS = 0.9
const STAGGER_SECONDS = 0.14
const EXIT_SECONDS = 0.35
const RISE_PX = 48 // motion.enterDrop in the product

// Centred logo, title and tagline. Each line drops in after the one above it.
export function titleCard(options: TitleCardOptions): Part {
  const el = element('div', 'part title-card')
  const lines: HTMLElement[] = []

  if (options.logo) {
    const logo = element('img', 'title-card-logo', el)
    logo.src = logoUrl
    lines.push(logo)
  }
  const title = element('h1', 'title-card-title', el)
  title.textContent = tr(options.title)
  lines.push(title)
  if (options.tagline) {
    const tagline = element('p', 'title-card-tagline', el)
    tagline.textContent = tr(options.tagline)
    lines.push(tagline)
  }

  return {
    el,
    render(time) {
      lines.forEach((line, index) => {
        const progress = span(time, options.from + index * STAGGER_SECONDS, ENTER_SECONDS, 'outExpo')
        line.style.opacity = String(progress)
        line.style.transform = `translateY(${(1 - progress) * RISE_PX}px)`
      })
      el.style.opacity = String(1 - span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic'))
    },
  }
}
