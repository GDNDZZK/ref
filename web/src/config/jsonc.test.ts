import { describe, expect, it } from 'vitest'
import { parseJsonc, parseJsoncObject } from './jsonc.ts'
import {
  parseSkillsPaths,
  patchSkillsPaths,
  renameObjectKey,
  splitShellWords,
  uniqueObjectKey,
} from './sectionHelpers.ts'

describe('parseJsonc', () => {
  it('strips UTF-8 BOM, line comments, block comments, and trailing commas while keeping strings intact', () => {
    const parsed = parseJsonc(`\uFEFF{
  // line comment
  "a": 1, /* block
  comment */ "b": [1, 2, 3,],
  "url": "https://x.y/z", // trailing
  "glob": "src/**/*.rs /* not a comment */",
  "esc": "quote \\" // still string",
  "nested": { "x": true, // comment before brace
  },
}`) as Record<string, unknown>

    expect(parsed).toEqual({
      a: 1,
      b: [1, 2, 3],
      url: 'https://x.y/z',
      glob: 'src/**/*.rs /* not a comment */',
      esc: 'quote " // still string',
      nested: { x: true },
    })
  })

  it('preserves object key order and rejects invalid syntax or non-object roots', () => {
    const ok = parseJsoncObject('{"z": 1, "a": 2, "m": 3,}')
    expect(ok.ok).toBe(true)
    if (ok.ok) expect(Object.keys(ok.value)).toEqual(['z', 'a', 'm'])

    const arr = parseJsoncObject('[1, 2, 3]')
    expect(arr).toMatchObject({ ok: false, rootNotObject: true })

    const broken = parseJsoncObject('{"a": }')
    expect(broken.ok).toBe(false)
  })
})

describe('sectionHelpers', () => {
  it('splits shell command lines with single quotes, double quotes, and escapes', () => {
    expect(splitShellWords('npx -y "@modelcontextprotocol/server" \'my dir\' a\\ b')).toEqual([
      'npx',
      '-y',
      '@modelcontextprotocol/server',
      'my dir',
      'a b',
    ])
  })

  it('reads and writes skills paths losslessly for both array and object forms', () => {
    const asArray = { skills: ['./skills'], untouched: 42 }
    expect(parseSkillsPaths(asArray)).toEqual(['./skills'])
    expect(patchSkillsPaths(asArray, ['./skills', '~/.ref/skills'])).toEqual({
      skills: ['./skills', '~/.ref/skills'],
      untouched: 42,
    })

    const asObj = { skills: { paths: ['./one'], customFlag: true } }
    expect(parseSkillsPaths(asObj)).toEqual(['./one'])
    expect(patchSkillsPaths(asObj, ['./two'])).toEqual({
      skills: { paths: ['./two'], customFlag: true },
    })
    expect(patchSkillsPaths({ skills: { paths: ['./one'] } }, [])).toEqual({})
  })

  it('renames object keys in place and generates unique keys', () => {
    const renamed = renameObjectKey({ first: 1, old: 2, third: 3 }, 'old', 'new')
    expect(Object.keys(renamed)).toEqual(['first', 'new', 'third'])
    expect(uniqueObjectKey({ mcp: {}, 'mcp-2': {} }, 'mcp')).toBe('mcp-3')
  })
})
