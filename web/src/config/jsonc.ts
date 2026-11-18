// Client-side JSONC parser matching `crates/config/src/jsonc.rs`:
// strips UTF-8 BOM, `//` and `/* */` comments outside double-quoted strings, and trailing commas
// before `}` or `]`, while preserving line/column positions for syntax error messages.

import { describeError } from '../errorMessage.ts'
import { isPlainObject, type JsonObject } from './jsonPatch.ts'

const UTF8_BOM = '\uFEFF'

function stripComments(source: string): string {
  const chars = Array.from(source)
  const out: string[] = []
  let index = 0
  let inString = false

  while (index < chars.length) {
    const ch = chars[index]!
    if (inString) {
      out.push(ch)
      if (ch === '\\' && index + 1 < chars.length) {
        out.push(chars[index + 1]!)
        index += 2
        continue
      }
      if (ch === '"') inString = false
      index += 1
      continue
    }

    if (ch === '"') {
      inString = true
      out.push(ch)
      index += 1
      continue
    }

    if (ch === '/' && chars[index + 1] === '/') {
      index = consumeLineComment(chars, index, out)
      continue
    }

    if (ch === '/' && chars[index + 1] === '*') {
      index = consumeBlockComment(chars, index, out)
      continue
    }

    out.push(ch)
    index += 1
  }

  return out.join('')
}

function consumeLineComment(chars: readonly string[], start: number, out: string[]): number {
  let index = start
  while (index < chars.length && chars[index] !== '\n') {
    out.push(' ')
    index += 1
  }
  return index
}

function consumeBlockComment(chars: readonly string[], start: number, out: string[]): number {
  out.push(' ', ' ')
  let index = start + 2
  while (index < chars.length && !(chars[index] === '*' && chars[index + 1] === '/')) {
    out.push(chars[index] === '\n' ? '\n' : ' ')
    index += 1
  }
  if (index < chars.length) {
    out.push(' ', ' ')
    index += 2
  }
  return index
}

function nextNonWhitespace(chars: readonly string[], start: number): string | undefined {
  for (let index = start; index < chars.length; index += 1) {
    const ch = chars[index]!
    if (!/\s/.test(ch)) return ch
  }
  return undefined
}

function stripTrailingCommas(source: string): string {
  const chars = Array.from(source)
  const out: string[] = []
  let index = 0
  let inString = false

  while (index < chars.length) {
    const ch = chars[index]!
    if (inString) {
      out.push(ch)
      if (ch === '\\' && index + 1 < chars.length) {
        out.push(chars[index + 1]!)
        index += 2
        continue
      }
      if (ch === '"') inString = false
      index += 1
      continue
    }

    if (ch === '"') {
      inString = true
      out.push(ch)
      index += 1
      continue
    }

    if (ch === ',') {
      const next = nextNonWhitespace(chars, index + 1)
      if (next === '}' || next === ']') {
        out.push(' ')
        index += 1
        continue
      }
    }

    out.push(ch)
    index += 1
  }

  return out.join('')
}

export function parseJsonc(text: string): unknown {
  const withoutBom = text.startsWith(UTF8_BOM) ? text.slice(1) : text
  const withoutComments = stripComments(withoutBom)
  const cleaned = stripTrailingCommas(withoutComments)
  return JSON.parse(cleaned)
}

export type JsoncObjectResult =
  { ok: true; value: JsonObject } | { ok: false; error: string; rootNotObject?: boolean }

export function parseJsoncObject(text: string): JsoncObjectResult {
  let parsed: unknown
  try {
    parsed = parseJsonc(text)
  } catch (error) {
    return { ok: false, error: describeError(error) }
  }
  if (!isPlainObject(parsed)) {
    return { ok: false, error: 'Configuration root must be a JSON object ({ ... }).', rootNotObject: true }
  }
  return { ok: true, value: parsed }
}
