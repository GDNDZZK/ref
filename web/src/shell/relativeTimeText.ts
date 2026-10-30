// A RelativeTime as words in the interface language.

import type { RelativeTime } from './sessionRow.ts'

type Translate = (key: string, named?: Record<string, unknown>) => string

export function relativeTimeText(time: RelativeTime, t: Translate, locale: string): string {
  switch (time.kind) {
    case 'justNow':
      return t('time.justNow')
    case 'minutes':
      return t('time.minutesAgo', { n: time.count })
    case 'hours':
      return t('time.hoursAgo', { n: time.count })
    case 'yesterday':
      return t('time.yesterday')
    case 'date':
      return new Intl.DateTimeFormat(locale, {
        month: 'short',
        day: 'numeric',
        ...(time.withYear ? { year: 'numeric' } : {}),
      }).format(time.date)
  }
}
