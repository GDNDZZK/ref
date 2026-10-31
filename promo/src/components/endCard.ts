import markUrl from '../../../assets/app/ref.svg'
import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export const REPOSITORY = 'github.com/GDNDZZK/ref'
export const PLATFORMS = 'Linux · macOS · Windows · GPL-3.0'
const FOOTNOTE: Localized = {
  en: 'Product screens come from a scripted demo backend. Cards and diagrams are drawn.',
  zh: '产品画面来自脚本化的演示后端，卡片与图示为绘制。',
}

export interface EndCardOptions {
  // The platform and licence line.
  platforms?: boolean
  // One extra line under the address (C7).
  note?: string | Localized
  from: number
  to: number
}

const ENTER_SECONDS = 0.9
const STAGGER_SECONDS = 0.12
const RISE_PX = 40
const EXIT_SECONDS = 0.35

// N7: the large mark, the repository address, optional lines, and the footnote on what is drawn.
export function endCard(options: EndCardOptions): Part {
  const el = element('div', 'part end-card')
  const lines: HTMLElement[] = []
  const mark = element('img', 'end-card-mark', el)
  mark.src = markUrl
  lines.push(mark)
  const address = element('div', 'end-card-address', el)
  address.textContent = REPOSITORY
  lines.push(address)
  if (options.platforms) {
    const platforms = element('div', 'end-card-line', el)
    platforms.textContent = PLATFORMS
    lines.push(platforms)
  }
  if (options.note) {
    const note = element('div', 'end-card-line', el)
    note.textContent = tr(options.note)
    lines.push(note)
  }
  const footnote = element('div', 'end-card-footnote', el)
  footnote.textContent = tr(FOOTNOTE)

  return {
    el,
    render(time) {
      lines.forEach((line, index) => {
        const progress = span(time, options.from + index * STAGGER_SECONDS, ENTER_SECONDS, 'outExpo')
        line.style.opacity = String(progress)
        line.style.transform = `translateY(${(1 - progress) * RISE_PX}px)`
      })
      footnote.style.opacity = String(span(time, options.from + 0.4, ENTER_SECONDS, 'outExpo'))
      el.style.opacity = String(1 - span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic'))
    },
  }
}
