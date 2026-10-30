// Syntax highlighting with Shiki, which runs without WebAssembly or eval: the JavaScript regex
// engine and token output only, so the page's CSP stays strict. Colors come out as CSS custom
// properties for both themes (--shiki-light, --shiki-dark), so switching theme re-tokenizes nothing.

import { createHighlighterCore } from 'shiki/core'
import type { HighlighterCore } from 'shiki/core'
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript'
import { BREEZE_DARK, BREEZE_LIGHT } from './breeze.ts'
import { languageFromName, loadLanguage } from './languages.ts'
import type { LanguageId } from './languages.ts'

export interface HighlightToken {
  content: string
  style: Record<string, string> // custom properties for the two themes
}

export interface Highlighted {
  tokens: HighlightToken[] // all lines in order, with "\n" tokens between lines
  text: string // what the tokens spell
}

const THEMES = { light: 'breeze-light', dark: 'breeze-dark' }
const MAX_CHARS = 200_000 // beyond this, plain text: tokenizing it would stall the page

let highlighter: Promise<HighlighterCore> | undefined
const loaded = new Set<LanguageId>()

function core(): Promise<HighlighterCore> {
  highlighter ??= createHighlighterCore({
    themes: [BREEZE_LIGHT, BREEZE_DARK],
    langs: [],
    engine: createJavaScriptRegexEngine({ forgiving: true }),
  })
  return highlighter
}

// Tokens for `code` in the language named by a fence's info string, or undefined when there is no
// highlighting for it. Loads the grammar on first use.
export async function highlight(code: string, languageName: string): Promise<Highlighted | undefined> {
  const language = languageFromName(languageName)
  if (!language || code.length > MAX_CHARS) return undefined

  const shiki = await core()
  if (!loaded.has(language)) {
    await shiki.loadLanguage(await loadLanguage(language))
    loaded.add(language)
  }
  const { tokens: lines } = shiki.codeToTokens(code, { lang: language, themes: THEMES, defaultColor: false })

  const tokens: HighlightToken[] = []
  lines.forEach((line, index) => {
    if (index > 0) tokens.push({ content: '\n', style: {} })
    for (const token of line) tokens.push({ content: token.content, style: token.htmlStyle ?? {} })
  })
  return { tokens, text: tokens.map((token) => token.content).join('') }
}
