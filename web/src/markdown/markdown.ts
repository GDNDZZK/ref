// Markdown -> HTML for assistant prose, reasoning and plain messages.
//
// CommonMark plus GFM tables, strikethrough, task lists and bare-URL autolinks (what Qt's
// GitHub dialect gives the GUI). Raw HTML in the source is shown as text, never rendered. The
// output carries no `style` attributes and no images, so it also passes a strict CSP:
// table alignment becomes a class and an image becomes a link to its source.

import MarkdownIt from 'markdown-it'
import type { Token } from 'markdown-it'

const TASK_MARKER = /^\[([ xX])\] +/
const ALIGN_STYLE = /^text-align:\s*(left|center|right)$/

const markdown = new MarkdownIt({ html: false, linkify: true, typographer: false })
markdown.linkify.set({ fuzzyLink: false, fuzzyEmail: false })

function addLinkAttributes(token: Token): void {
  token.attrSet('target', '_blank')
  token.attrSet('rel', 'noopener noreferrer')
}

const defaultLinkOpen =
  markdown.renderer.rules.link_open ??
  ((tokens, index, options, _env, self) => self.renderToken(tokens, index, options))
markdown.renderer.rules.link_open = (tokens, index, options, env, self) => {
  addLinkAttributes(tokens[index]!)
  return defaultLinkOpen(tokens, index, options, env, self)
}

// ![alt](src) -> a link to src. Remote images are blocked by the CSP anyway.
markdown.renderer.rules.image = (tokens, index) => {
  const token = tokens[index]!
  const source = String(token.attrGet('src') ?? '')
  const href = markdown.utils.escapeHtml(source)
  const label = markdown.utils.escapeHtml(token.content || source)
  return `<a href="${href}" target="_blank" rel="noopener noreferrer">${label}</a>`
}

// `- [x] done` -> a disabled checkbox in front of the item's text.
function taskLists(state: {
  tokens: Token[]
  Token: new (type: string, tag: string, nesting: 0 | 1 | -1) => Token
}): void {
  const { tokens } = state
  for (let i = 2; i < tokens.length; i++) {
    const inline = tokens[i]!
    const opensItemParagraph =
      inline.type === 'inline' &&
      tokens[i - 1]!.type === 'paragraph_open' &&
      tokens[i - 2]!.type === 'list_item_open'
    if (!opensItemParagraph) continue

    const first = inline.children?.[0]
    const marker = first?.type === 'text' ? TASK_MARKER.exec(first.content) : null
    if (!first || !marker) continue

    first.content = first.content.slice(marker[0].length)
    const checkbox = new state.Token('html_inline', '', 0)
    checkbox.content = `<input class="task-checkbox" type="checkbox" disabled${marker[1] === ' ' ? '' : ' checked'}> `
    inline.children!.unshift(checkbox)
    tokens[i - 2]!.attrJoin('class', 'task-list-item')
  }
}
markdown.core.ruler.after('inline', 'task-lists', taskLists)

// Table cell alignment: `style="text-align:center"` -> `class="align-center"`.
function alignmentClasses(state: { tokens: Token[] }): void {
  for (const token of state.tokens) {
    const isCell = token.type === 'th_open' || token.type === 'td_open'
    const style = isCell ? (token.attrGet('style') as string | null) : null
    const align = style ? ALIGN_STYLE.exec(style)?.[1] : undefined
    if (!align) continue
    token.attrs = (token.attrs ?? []).filter(([name]) => name !== 'style')
    token.attrSet('class', `align-${align}`)
  }
}
markdown.core.ruler.push('align-classes', alignmentClasses)

export function renderMarkdown(source: string): string {
  return markdown.render(source)
}
