import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Lang } from '../../src/engine/i18n.ts'
import { REPO_ROOT } from './binaries.ts'

const LOCALE_FILES: Record<Lang, string> = { en: 'en.json', zh: 'zh-CN.json' }
const LOCALES_DIR = join(REPO_ROOT, 'web', 'src', 'i18n', 'locales')

// Interface texts from the web page's locale files, e.g. phrase('en', 'sidebar.needsPermission').
// The GUI shows the same words (its Chinese translation imports the web's).
export function phrase(lang: Lang, key: string): string {
  const locale = JSON.parse(readFileSync(join(LOCALES_DIR, LOCALE_FILES[lang]), 'utf8')) as Record<string, unknown>
  const value = key.split('.').reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], locale)
  if (typeof value !== 'string') throw new Error(`no text "${key}" in ${LOCALE_FILES[lang]}`)
  return value
}
