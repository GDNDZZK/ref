// Splits prompt text into plain segments and `[Image N]` attachment tokens.

export interface TextSegment {
  text: string
  isImageToken: boolean
}

const IMAGE_TOKEN_GLOBAL = /\[Image \d+\]/g
const IMAGE_TOKEN_TEST = /\[Image \d+\]/

export function hasImageTokens(text: string): boolean {
  return IMAGE_TOKEN_TEST.test(text)
}

export function splitImageTokens(text: string): TextSegment[] {
  if (text === '') return []
  const segments: TextSegment[] = []
  let lastIndex = 0

  for (const match of text.matchAll(IMAGE_TOKEN_GLOBAL)) {
    const index = match.index
    if (index > lastIndex) {
      segments.push({ text: text.slice(lastIndex, index), isImageToken: false })
    }
    segments.push({ text: match[0], isImageToken: true })
    lastIndex = index + match[0].length
  }

  if (lastIndex < text.length) {
    segments.push({ text: text.slice(lastIndex), isImageToken: false })
  }
  return segments
}
