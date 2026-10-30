import { describe, expect, it } from 'vitest'
import { renderMarkdown } from './markdown.ts'

describe('renderMarkdown', () => {
  it('renders CommonMark', () => {
    expect(renderMarkdown('# Title\n\nSome *emphasis* and `code`.')).toBe(
      '<h1>Title</h1>\n<p>Some <em>emphasis</em> and <code>code</code>.</p>\n',
    )
  })

  it('shows raw HTML as text', () => {
    const html = renderMarkdown('<script>alert(1)</script>\n\n<b>x</b>')
    expect(html).not.toContain('<script>')
    expect(html).toContain('&lt;script&gt;')
    expect(html).not.toContain('<b>')
  })

  it('refuses javascript: links', () => {
    expect(renderMarkdown('[x](javascript:alert(1))')).not.toContain('href')
  })

  it('opens links in a new tab without opener', () => {
    expect(renderMarkdown('[a](https://example.com)')).toContain(
      '<a href="https://example.com" target="_blank" rel="noopener noreferrer">a</a>',
    )
  })

  it('links bare URLs but not bare domains', () => {
    expect(renderMarkdown('see https://example.com/x now')).toContain('<a href="https://example.com/x"')
    expect(renderMarkdown('see example.com now')).not.toContain('<a ')
  })

  it('renders GFM strikethrough', () => {
    expect(renderMarkdown('~~gone~~')).toContain('<s>gone</s>')
  })

  it('renders tables with alignment classes instead of style attributes', () => {
    const html = renderMarkdown('| a | b |\n|:-:|--:|\n| 1 | 2 |')
    expect(html).toContain('<table>')
    expect(html).toContain('<th class="align-center">a</th>')
    expect(html).toContain('<td class="align-right">2</td>')
    expect(html).not.toContain('style=')
  })

  it('renders task lists as disabled checkboxes', () => {
    const html = renderMarkdown('- [x] done\n- [ ] todo\n- plain')
    expect(html).toContain(
      '<li class="task-list-item"><input class="task-checkbox" type="checkbox" disabled checked> done</li>',
    )
    expect(html).toContain('<input class="task-checkbox" type="checkbox" disabled> todo')
    expect(html).toContain('<li>plain</li>')
  })

  it('turns images into links', () => {
    expect(renderMarkdown('![logo](https://example.com/a.png)')).toContain(
      '<a href="https://example.com/a.png"',
    )
    expect(renderMarkdown('![logo](https://example.com/a.png)')).not.toContain('<img')
  })
})
