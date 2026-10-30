import { describe, expect, it } from 'vitest'
import {
  isWebUrl,
  limitLines,
  parseDiff,
  parseFound,
  parseSourceListing,
  parseTodos,
  peerName,
  tailLines,
  toolDetail,
  toolIcon,
} from './toolContent.ts'

describe('toolDetail', () => {
  it('reads the line a tool is known by', () => {
    expect(toolDetail('bash', { command: 'ls -la\necho hi' })).toBe('$ ls -la')
    expect(toolDetail('grep', { pattern: 'foo', path: 'src' })).toBe('"foo" in src')
    expect(toolDetail('edit', { path: 'a.ts', edits: [{}, {}] })).toBe('a.ts (2 edits)')
    expect(toolDetail('webfetch', { url: 'https://x.dev', format: 'text' })).toBe('https://x.dev (text)')
    expect(toolDetail('send_message', { to: 'helper' })).toBe('→ helper')
  })

  it('counts finished todos', () => {
    const todos = [
      { content: 'a', status: 'completed' },
      { content: 'b', status: 'pending' },
    ]
    expect(toolDetail('todo', { todos })).toBe('1/2 done')
  })

  it('lists the first plain arguments of an unknown tool', () => {
    expect(toolDetail('srv_lookup', { q: 'x', deep: { a: 1 }, n: 3, ok: true, more: 1 })).toBe(
      'q=x n=3 ok=true',
    )
  })
})

describe('toolIcon and peerName', () => {
  it('picks an icon per tool, MCP tools by their server prefix', () => {
    expect(toolIcon('bash')).toBe('terminal-16')
    expect(toolIcon('github_create_issue')).toBe('mcp-16')
    expect(toolIcon('mystery')).toBe('tool-16')
  })

  it('names the session a cross-session tool talks to', () => {
    expect(peerName('send_message', { to: 'a' })).toBe('a')
    expect(peerName('read_session', { session: 'b' })).toBe('b')
    expect(peerName('bash', { command: 'x' })).toBeUndefined()
  })
})

describe('parseTodos', () => {
  it('keeps the known statuses and defaults to pending', () => {
    const todos = parseTodos({
      todos: [
        { content: ' a ', status: 'in_progress' },
        { content: 'b' },
        { content: '', status: 'completed' },
      ],
    })
    expect(todos).toEqual([
      { content: 'a', status: 'in_progress' },
      { content: 'b', status: 'pending' },
    ])
  })
})

describe('parseDiff', () => {
  it('classifies the rows and cuts the sign off', () => {
    const rows = parseDiff('--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n same\n')
    expect(rows.map((row) => row.kind)).toEqual(['file', 'file', 'hunk', 'removed', 'added', 'context'])
    expect(rows.slice(3).map((row) => `${row.sign}${row.text}`)).toEqual(['-old', '+new', 'same'])
  })
})

describe('parseSourceListing', () => {
  it('reads a file with numbered lines and the notes after it', () => {
    const listing = parseSourceListing('[a.txt#AB12]\n1:hello\n2:\n3:world\n\n[2 more lines in file.]')
    expect(listing?.files).toEqual([
      {
        path: 'a.txt',
        tag: 'AB12',
        rows: [
          { number: 1, text: 'hello', match: false },
          { number: 2, text: '', match: false },
          { number: 3, text: 'world', match: false },
        ],
      },
    ])
    expect(listing?.notes).toEqual(['[2 more lines in file.]'])
  })

  it('reads grep groups with matches, context and gaps', () => {
    const listing = parseSourceListing(
      '[a.rs#1111]\n 1:use x;\n*2:fn foo() {}\n...\n*9:foo();\n\n[b.rs#2222]\n*4:foo\n\n[Match limit of 5 reached]',
    )
    expect(listing?.files.map((file) => file.path)).toEqual(['a.rs', 'b.rs'])
    expect(listing?.files[0]?.rows).toEqual([
      { number: 1, text: 'use x;', match: false },
      { number: 2, text: 'fn foo() {}', match: true },
      'gap',
      { number: 9, text: 'foo();', match: true },
    ])
    expect(listing?.notes).toEqual(['[Match limit of 5 reached]'])
  })

  it('gives up on output in another format', () => {
    expect(parseSourceListing('No matches found.')).toBeUndefined()
    expect(parseSourceListing('Directory src/ (2 entries)\na.rs\nb/')).toBeUndefined()
  })
})

describe('parseFound', () => {
  it('splits paths into directory and name', () => {
    const { paths, notes } = parseFound('src/a.ts\nsrc/lib/\nb.md\n\n[Showing 3 of 9 results.]')
    expect(paths).toEqual([
      { directory: 'src/', name: 'a.ts', isDir: false },
      { directory: 'src/', name: 'lib/', isDir: true },
      { directory: '', name: 'b.md', isDir: false },
    ])
    expect(notes).toEqual(['[Showing 3 of 9 results.]'])
  })

  it('has no paths when nothing was found', () => {
    expect(parseFound('No files found matching `x` in .').paths).toEqual([])
  })
})

describe('limitLines and tailLines', () => {
  it('cuts long text and says how much is left', () => {
    expect(limitLines('a\nb\nc\n', 2)).toEqual({ text: 'a\nb', hidden: 1 })
    expect(limitLines('a\nb', 5)).toEqual({ text: 'a\nb', hidden: 0 })
    expect(limitLines('a\nb\nc', undefined).hidden).toBe(0)
    expect(tailLines('a\nb\nc\nd', 2)).toEqual({ text: 'c\nd', hidden: 2 })
  })
})

describe('isWebUrl', () => {
  it('accepts http and https only', () => {
    expect(isWebUrl('https://example.com/a?b=1')).toBe(true)
    expect(isWebUrl('http://localhost:3000')).toBe(true)
    expect(isWebUrl('javascript:alert(1)')).toBe(false)
    expect(isWebUrl('data:text/html,<b>x</b>')).toBe(false)
    expect(isWebUrl('file:///etc/passwd')).toBe(false)
    expect(isWebUrl('not a url')).toBe(false)
  })
})
