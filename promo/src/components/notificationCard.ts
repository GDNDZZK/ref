import appIconUrl from '../../../assets/app/png/ref-128.png'
import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface NotificationCardOptions {
  title: string | Localized
  body: string | Localized
  from: number
  to: number
}

const ENTER_SECONDS = 0.7
const EXIT_SECONDS = 0.4
const OFFSCREEN_PX = 520

// A desktop notification, drawn: the operating system paints the real one, and a headless
// run cannot capture it. Slides in from the right edge.
export function notificationCard(options: NotificationCardOptions): Part {
  const el = element('div', 'part notification-card')
  const icon = element('img', 'notification-icon', el)
  icon.src = appIconUrl
  const text = element('div', 'notification-text', el)
  element('div', 'notification-app', text).textContent = 'ref'
  element('div', 'notification-title', text).textContent = tr(options.title)
  element('div', 'notification-body', text).textContent = tr(options.body)

  return {
    el,
    render(time) {
      const entering = span(time, options.from, ENTER_SECONDS, 'outBack')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.transform = `translateX(${(1 - entering + leaving) * OFFSCREEN_PX}px)`
    },
  }
}
