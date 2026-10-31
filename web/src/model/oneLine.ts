// Single-line text, as the terminal client's one_line: Unicode white space (U+0085 included) is a
// separator that collapses to one space, other control characters go, a byte order mark stays.

const CONTROL = /^\p{Cc}$/u
const NEXT_LINE = '\u0085'
const BYTE_ORDER_MARK = '\ufeff'

function isSpace(character: string): boolean {
  return character === NEXT_LINE || (character !== BYTE_ORDER_MARK && /^\s$/.test(character))
}

export function oneLine(text: string): string {
  let result = ''
  let space = false
  for (const character of text) {
    if (isSpace(character)) {
      space = true
    } else if (!CONTROL.test(character)) {
      if (space && result !== '') result += ' '
      space = false
      result += character
    }
  }
  return result
}
