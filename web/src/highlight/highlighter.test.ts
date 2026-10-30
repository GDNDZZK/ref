import { describe, expect, it } from 'vitest'
import { highlight } from './highlighter.ts'
import { languageFromName, languageFromPath } from './languages.ts'

describe('language names', () => {
  it('knows only its own names, not those of Object.prototype', () => {
    for (const name of ['constructor', '__proto__', 'toString', 'hasOwnProperty']) {
      expect(languageFromName(name), name).toBeUndefined()
    }
  })
})

describe('languages', () => {
  it('resolves fence info strings and aliases', () => {
    expect(languageFromName('rust')).toBe('rust')
    expect(languageFromName('RS')).toBe('rust')
    expect(languageFromName('sh')).toBe('bash')
    expect(languageFromName('yml')).toBe('yaml')
    expect(languageFromName('')).toBeUndefined()
    expect(languageFromName('brainfuck')).toBeUndefined()
  })

  it('resolves files by extension and name', () => {
    expect(languageFromPath('src/main.rs')).toBe('rust')
    expect(languageFromPath('/a/b/Dockerfile')).toBe('dockerfile')
    expect(languageFromPath('CMakeLists.txt')).toBe('cmake')
    expect(languageFromPath('README')).toBeUndefined()
  })
})

describe('highlight', () => {
  it('spells the code back and colors it for both themes', async () => {
    const code = 'fn main() {\n    let x = 42; // answer\n}\n'
    const result = await highlight(code, 'rust')
    expect(result?.text).toBe(code)
    const keyword = result?.tokens.find((token) => token.content === 'fn')
    expect(keyword?.style['--shiki-light']).toBeDefined()
    expect(keyword?.style['--shiki-dark']).toBeDefined()
    const comment = result?.tokens.find((token) => token.content.includes('answer'))
    expect(comment?.style['--shiki-light']?.toLowerCase()).toBe('#898887')
    expect(comment?.style['--shiki-dark']?.toLowerCase()).toBe('#7a7c7d')
  })

  it('colors strings and bold keywords as Breeze does', async () => {
    const result = await highlight('let s = "hi";', 'rust')
    const string = result?.tokens.find((token) => token.content.includes('"hi"'))
    expect(string?.style['--shiki-light']?.toLowerCase()).toBe('#bf0303')
    expect(string?.style['--shiki-dark']?.toLowerCase()).toBe('#f44f4f')
    const keyword = result?.tokens.find((token) => token.content.trim() === 'let')
    expect(keyword?.style['--shiki-light-font-weight']).toBe('bold')
  })

  it('has no tokens for unknown languages', async () => {
    expect(await highlight('x', 'brainfuck')).toBeUndefined()
    expect(await highlight('x', '')).toBeUndefined()
  })

  it.each(['python', 'typescript', 'bash', 'json', 'yaml', 'html', 'sql', 'diff', 'markdown'])(
    'tokenizes %s',
    async (language) => {
      const code = language === 'json' ? '{"a": [1, true]}' : 'one two\nthree'
      expect((await highlight(code, language))?.text).toBe(code)
    },
  )
})
