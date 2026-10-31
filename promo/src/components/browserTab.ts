import alertIconUrl from '../../../assets/app/web/favicon-alert.svg'
import iconUrl from '../../../assets/app/web/favicon.svg'
import { span } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'

export interface BrowserTabOptions {
  title: string
  // Time at which the page asks for attention: dot before the title, alert icon.
  alertAt: number
  from: number
  to: number
}

const ENTER_SECONDS = 0.5
const EXIT_SECONDS = 0.3
const ALERT_TITLE_PREFIX = '● ' // ALERT_TITLE_PREFIX in web/src/notify/attention.ts

// A browser tab as ref-web paints it: the page's own favicon and title, switched to the
// alert versions by `alertAt`. Drawn, since a headless page has no tab strip.
export function browserTab(options: BrowserTabOptions): Part {
  const el = element('div', 'part browser-tab')
  const icon = element('img', 'browser-tab-icon', el)
  icon.src = iconUrl
  const alertIcon = element('img', 'browser-tab-icon', el)
  alertIcon.src = alertIconUrl
  const title = element('span', 'browser-tab-title', el)

  return {
    el,
    render(time) {
      const alerting = time >= options.alertAt
      icon.style.display = alerting ? 'none' : ''
      alertIcon.style.display = alerting ? '' : 'none'
      title.textContent = (alerting ? ALERT_TITLE_PREFIX : '') + options.title
      el.dataset.alert = alerting ? 'on' : 'off'
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
    },
  }
}
