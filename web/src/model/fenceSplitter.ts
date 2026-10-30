// Splits streamed Markdown into prose and fenced code blocks.
// Ported from qutie's FenceSplitter (gui/src/ui/fence_splitter.cpp); keep the two in step.
//
// Follows the CommonMark fenced code block rules: a fence is a run of at least three
// ``` or ~~~, indented by at most three spaces; the first word of the info string is
// the language; the closing fence uses the same marker and is at least as long.
//
// Text is released as soon as it is unambiguous. Only a line that may still turn into
// a fence is held back until it ends. Line breaks are released lazily, so a block never
// ends with the line breaks that separate it from the next one.

export type FenceEvent =
  | { type: 'codeOpened'; language: string } // empty when the fence has no info string
  | { type: 'codeClosed' }
  | { type: 'text'; text: string } // belongs to the open code block, or to prose outside one

interface Fence {
  marker: string
  length: number
  indent: number
  language: string
}

const MAX_FENCE_INDENT = 3
const MIN_FENCE_LENGTH = 3

function countLeading(text: string, char: string): number {
  let count = 0
  while (text[count] === char) count++
  return count
}

function isFenceMarker(char: string): boolean {
  return char === '`' || char === '~'
}

// Also accepts '\r', so a fence line ending in CRLF still closes the block.
function isBlank(text: string): boolean {
  return /^\s*$/.test(text)
}

function parseOpeningFence(line: string): Fence | undefined {
  const indent = countLeading(line, ' ')
  if (indent > MAX_FENCE_INDENT || indent === line.length) return undefined

  const marker = line.charAt(indent)
  const length = isFenceMarker(marker) ? countLeading(line.slice(indent), marker) : 0
  if (length < MIN_FENCE_LENGTH) return undefined

  // Without this rule "```a```" at the start of a line, which is inline code, would open a block.
  const info = line.slice(indent + length).trim()
  if (marker === '`' && info.includes('`')) return undefined

  const language = info.split(/\s/, 1)[0] ?? ''
  return { marker, length, indent, language }
}

function closes(fence: Fence, line: string): boolean {
  const indent = countLeading(line, ' ')
  if (indent > MAX_FENCE_INDENT) return false

  const rest = line.slice(indent)
  const length = countLeading(rest, fence.marker)
  return length >= fence.length && isBlank(rest.slice(length))
}

export class FenceSplitter {
  private openFence: Fence | undefined
  private heldLine = '' // start of the current line while it may still be a fence
  private lineReleased = false // the current line is plain content and already emitted in part
  private pendingLineBreaks = 0
  private atBlockStart = true // nothing emitted since the last fence or the start

  feed(delta: string): FenceEvent[] {
    const events: FenceEvent[] = []
    let position = 0

    for (;;) {
      const lineBreak = delta.indexOf('\n', position)
      if (lineBreak < 0) {
        this.appendToLine(delta.slice(position), events)
        return events
      }
      this.appendToLine(delta.slice(position, lineBreak), events)
      this.endLine(events)
      position = lineBreak + 1
    }
  }

  // Releases the held-back last line. Feeding afterwards is not supported.
  finish(): FenceEvent[] {
    const events: FenceEvent[] = []
    this.endLine(events)
    return events
  }

  private appendToLine(text: string, events: FenceEvent[]): void {
    if (this.lineReleased) {
      this.emitText(text, events)
      return
    }

    this.heldLine += text
    if (this.mayBecomeFence(this.heldLine)) return

    this.lineReleased = true
    this.emitText(this.stripCodeIndent(this.heldLine), events)
    this.heldLine = ''
  }

  private endLine(events: FenceEvent[]): void {
    const lineWasHeld = !this.lineReleased
    const line = this.heldLine
    this.lineReleased = false
    this.heldLine = ''

    if (lineWasHeld && this.openFence && closes(this.openFence, line)) {
      this.openFence = undefined
      this.pendingLineBreaks = 0
      this.atBlockStart = true
      events.push({ type: 'codeClosed' })
      return
    }

    if (lineWasHeld && !this.openFence) {
      this.openFence = parseOpeningFence(line)
      if (this.openFence) {
        this.pendingLineBreaks = 0
        this.atBlockStart = true
        events.push({ type: 'codeOpened', language: this.openFence.language })
        return
      }
    }

    if (lineWasHeld) this.emitText(this.stripCodeIndent(line), events)
    this.pendingLineBreaks++
  }

  // `partialLine` has not ended yet, so this asks whether some continuation could make it a fence.
  private mayBecomeFence(partialLine: string): boolean {
    const indent = countLeading(partialLine, ' ')
    if (indent > MAX_FENCE_INDENT) return false

    const rest = partialLine.slice(indent)
    if (rest === '') return true

    if (this.openFence) {
      const length = countLeading(rest, this.openFence.marker)
      return length > 0 && isBlank(rest.slice(length))
    }

    const marker = rest.charAt(0)
    if (!isFenceMarker(marker)) return false

    // A complete opening run is held until the line ends: the info string decides the rest.
    const length = countLeading(rest, marker)
    return length >= MIN_FENCE_LENGTH || length === rest.length
  }

  // Inside a code block, removes up to as many leading spaces as the opening fence was indented.
  private stripCodeIndent(line: string): string {
    if (!this.openFence) return line
    return line.slice(Math.min(countLeading(line, ' '), this.openFence.indent))
  }

  private emitText(text: string, events: FenceEvent[]): void {
    if (text === '') return

    // Blank lines before the first prose line separate it from a fence; leading blank
    // lines inside a code block are content.
    const startsProse = this.atBlockStart && !this.openFence
    this.atBlockStart = false
    const lineBreaks = this.pendingLineBreaks
    this.pendingLineBreaks = 0
    const released = (startsProse ? '' : '\n'.repeat(lineBreaks)) + text

    const previous = events.at(-1)
    if (previous?.type === 'text') {
      previous.text += released
      return
    }
    events.push({ type: 'text', text: released })
  }
}
