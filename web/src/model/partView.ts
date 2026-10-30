// The text of one message part as shown so far. The shown text trails the real text while the
// pacer releases it; a text part is also cut into markdown and code segments as it grows.

import { FenceSplitter } from './fenceSplitter.ts'
import type { FenceEvent } from './fenceSplitter.ts'

export interface Segment {
  kind: 'markdown' | 'code'
  language: string
  text: string
}

export class PartView {
  private shownText = ''
  private readonly splitsFences: boolean
  private splitter = new FenceSplitter()
  private readonly cut: Segment[] = []
  private open: Segment | undefined // the segment further text goes to
  private finished = false // the splitter's held-back line was released

  constructor(splitsFences: boolean) {
    this.splitsFences = splitsFences
  }

  get shown(): string {
    return this.shownText
  }

  get segments(): readonly Segment[] {
    return this.cut
  }

  // Replaces everything shown.
  setText(text: string): void {
    this.shownText = ''
    this.splitter = new FenceSplitter()
    this.cut.length = 0
    this.open = undefined
    this.finished = false
    this.append(text)
  }

  append(text: string): void {
    if (text === '') return
    if (this.finished) {
      this.setText(this.shownText + text)
      return
    }
    this.shownText += text
    if (this.splitsFences) this.place(this.splitter.feed(text))
  }

  // No more text for now: shows the line the splitter was unsure about.
  finish(): void {
    if (this.finished || !this.splitsFences) return
    this.finished = true
    this.place(this.splitter.finish())
  }

  private place(events: FenceEvent[]): void {
    for (const event of events) {
      if (event.type === 'codeOpened') {
        this.open = { kind: 'code', language: event.language, text: '' }
        this.cut.push(this.open)
      } else if (event.type === 'codeClosed') {
        this.open = undefined
      } else {
        // Prose after a closed fence starts a new markdown segment.
        if (!this.open) {
          this.open = { kind: 'markdown', language: '', text: '' }
          this.cut.push(this.open)
        }
        this.open.text += event.text
      }
    }
  }
}
