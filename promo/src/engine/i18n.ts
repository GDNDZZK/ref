export const LANGS = ['en', 'zh'] as const
export type Lang = (typeof LANGS)[number]
export type Localized = Readonly<Record<Lang, string>>

let currentLang: Lang = 'en'

export function isLang(value: string): value is Lang {
  return (LANGS as readonly string[]).includes(value)
}

export function setLang(lang: Lang): void {
  currentLang = lang
}

export function getLang(): Lang {
  return currentLang
}

// Scene copy is either one string (names, code) or both languages.
export function tr(text: string | Localized): string {
  return typeof text === 'string' ? text : text[currentLang]
}
