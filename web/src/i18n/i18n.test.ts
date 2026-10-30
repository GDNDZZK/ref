import { describe, expect, it } from 'vitest'
import { LOCALES, pickLocale } from './index.ts'

function keys(tree: Record<string, unknown>, prefix = ''): string[] {
  return Object.entries(tree).flatMap(([name, value]) =>
    typeof value === 'object' && value !== null
      ? keys(value as Record<string, unknown>, `${prefix}${name}.`)
      : [`${prefix}${name}`],
  )
}

describe('locales', () => {
  it('have the same keys', () => {
    expect(keys(LOCALES['zh-CN']).sort()).toEqual(keys(LOCALES.en).sort())
  })

  it('use the same placeholders', () => {
    const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()
    const flat = (tree: Record<string, unknown>) =>
      Object.fromEntries(
        keys(tree).map((key) => [
          key,
          key
            .split('.')
            .reduce<unknown>((node, part) => (node as Record<string, unknown>)[part], tree) as string,
        ]),
      )
    const english = flat(LOCALES.en)
    const chinese = flat(LOCALES['zh-CN'])
    for (const key of Object.keys(english)) {
      expect(placeholders(chinese[key]!), key).toEqual(placeholders(english[key]!))
    }
  })
})

describe('pickLocale', () => {
  it('follows the first language we have', () => {
    expect(pickLocale(['zh-TW', 'en'])).toBe('zh-CN')
    expect(pickLocale(['de', 'en-GB'])).toBe('en')
  })

  it('falls back to English', () => {
    expect(pickLocale(['de', 'fr'])).toBe('en')
    expect(pickLocale([])).toBe('en')
  })
})
