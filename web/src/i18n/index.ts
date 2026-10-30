import { createI18n } from 'vue-i18n'
import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

export const LOCALES = { en, 'zh-CN': zhCN } as const
export type LocaleName = keyof typeof LOCALES

const FALLBACK_LOCALE: LocaleName = 'en'

// The first of the browser's preferred languages that we have a translation for.
export function pickLocale(preferred: readonly string[]): LocaleName {
  for (const language of preferred) {
    if (language.toLowerCase().startsWith('zh')) return 'zh-CN'
    if (language.toLowerCase().startsWith('en')) return 'en'
  }
  return FALLBACK_LOCALE
}

let appI18n: ReturnType<typeof createAppI18n> | undefined

export function createAppI18n(preferred: readonly string[] = navigator.languages) {
  const created = createI18n({
    legacy: false,
    locale: pickLocale(preferred),
    fallbackLocale: FALLBACK_LOCALE,
    messages: LOCALES,
  })
  appI18n = created
  return created
}

// For text made outside components (toasts from stores).
export function translate(key: string, named: Record<string, unknown> = {}): string {
  if (!appI18n) throw new Error('the interface language has not been set up')
  return appI18n.global.t(key, named)
}
