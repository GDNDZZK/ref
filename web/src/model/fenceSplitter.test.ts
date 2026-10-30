import { describe, expect, it } from 'vitest'
import { FenceSplitter } from './fenceSplitter.ts'
import type { FenceEvent } from './fenceSplitter.ts'

// Flattens events into one string: fences become markers, text is concatenated.
// Chunking only changes how text is split across events, so equal renderings mean
// equal block contents.
function render(events: FenceEvent[]): string {
  return events
    .map((event) => {
      if (event.type === 'codeOpened') return `[${event.language}]`
      if (event.type === 'codeClosed') return '[/]'
      return event.text
    })
    .join('')
}

function splitWhole(input: string): string {
  const splitter = new FenceSplitter()
  const fed = render(splitter.feed(input))
  return fed + render(splitter.finish())
}

function splitPerCharacter(input: string): string {
  const splitter = new FenceSplitter()
  let rendered = ''
  for (const char of input) rendered += render(splitter.feed(char))
  return rendered + render(splitter.finish())
}

// [name, input, expected]
const cases: [string, string, string][] = [
  ['plain', 'Hello\nworld', 'Hello\nworld'],
  ['fence', 'a\n```cpp\nint x;\n```\nb', 'a[cpp]int x;[/]b'],
  ['tilde fence', '~~~\nx\n~~~', '[]x[/]'],
  ['unclosed fence', '```\nx', '[]x'],
  ['info string first word', '``` rust ignore\nx\n```', '[rust]x[/]'],

  ['blank lines around fence dropped', 'a\n\n```\nx\n```\n\nb', 'a[]x[/]b'],
  ['blank lines inside code kept', '```\n\nx\n\ny\n```', '[]\nx\n\ny[/]'],

  ['other marker is content', '~~~\n```\n~~~', '[]```[/]'],
  ['shorter closing is content', '````\n```\n`````', '[]```[/]'],
  ['closing with text is content', '```\n``` x\n```', '[]``` x[/]'],
  ['crlf', '```cpp\r\nx\r\n```\r\n', '[cpp]x\r[/]'],

  ['indented fence strips indent', '  ```py\n  x\n    y\n  ```', '[py]x\n  y[/]'],
  ['four spaces is not a fence', '    ```\nx', '    ```\nx'],
  ['inline code is not a fence', '```a``` b', '```a``` b'],
  ['short run is not a fence', '``x`` y', '``x`` y'],
]

describe('FenceSplitter', () => {
  it.each(cases)('splits %s', (_name, input, expected) => {
    expect(splitWhole(input)).toBe(expected)
  })

  it.each(cases)('splits %s one character at a time', (_name, input, expected) => {
    expect(splitPerCharacter(input)).toBe(expected)
  })

  it('releases prose before the line ends', () => {
    const splitter = new FenceSplitter()
    expect(render(splitter.feed('Hello wor'))).toBe('Hello wor')
    expect(render(splitter.feed('ld\nnext'))).toBe('ld\nnext')
  })

  it('holds a line that may become a fence', () => {
    const splitter = new FenceSplitter()
    expect(render(splitter.feed('``'))).toBe('')
    expect(render(splitter.feed('`cp'))).toBe('')
    expect(render(splitter.feed('p\nint'))).toBe('[cpp]int')
    expect(render(splitter.feed('\n`'))).toBe('')
    expect(render(splitter.feed('`x'))).toBe('\n``x')
  })
})
