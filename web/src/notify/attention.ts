// The tab asks for attention while the page is not in front: a dot before the title and the
// alert favicon (assets/app/web/favicon-alert.svg).

export const ALERT_TITLE_PREFIX = '● '
const FAVICON = '/favicon.svg'
const FAVICON_ALERT = '/favicon-alert.svg'

export interface Attention {
  // The title without the alert mark. Whoever knows what the tab shows (the session name, say)
  // sets it here instead of writing document.title.
  setBaseTitle(title: string): void
  setAlerting(alerting: boolean): void
}

export function documentAttention(page: Document): Attention {
  let baseTitle = page.title
  let alerting = false

  function apply(): void {
    page.title = alerting ? ALERT_TITLE_PREFIX + baseTitle : baseTitle
    const link = page.querySelector<HTMLLinkElement>('link[rel="icon"]')
    if (link) link.href = alerting ? FAVICON_ALERT : FAVICON
  }

  return {
    setBaseTitle(title) {
      baseTitle = title
      apply()
    },
    setAlerting(next) {
      if (next === alerting) return
      alerting = next
      apply()
    },
  }
}
