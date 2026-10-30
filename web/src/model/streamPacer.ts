// Evens out the pace of a streamed reply. Ported from qutie's StreamPacer
// (gui/src/ui/stream_pacer.cpp); keep the two in step.
//
// Servers send text in bursts: a token at a time, or a few hundred characters every couple of
// hundred milliseconds. Shown as it arrives, the second kind pops in a paragraph at a time.
//
// The pacer holds what arrived and lets it out at a steady rate: each arrival sets the rate so
// the backlog runs out just as the next arrival is due, judged by the recent gaps between
// arrivals. Shown text trails the stream by about one gap, at most MAX_LAG_MS.
//
//   arrives   ####          ####          ####
//   shown     \____________ \____________ \____________   steady, one gap behind
//
// Releases end on grapheme boundaries, so an emoji or a combined character never shows half-made.
// Time is in milliseconds and comes from the caller.

// `stream` names what the text belongs to (a message part); adjacent chunks of one stream merge.
export interface PacedText {
  stream: string
  text: string
}

const MIN_GAP_MS = 16 // about a frame: anything faster shows as it arrives
const MAX_LAG_MS = 200 // longest the shown text trails the stream
const BURST_WINDOW_MS = 4 // arrivals closer than this came in one network read
const GAP_SMOOTHING = 0.25 // weight of the newest gap in the expected one

const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

// Length of the longest start of `text` that is at most `budget` code units long and ends
// between two graphemes.
function graphemePrefix(text: string, budget: number): number {
  if (budget >= text.length) return text.length
  let prefix = 0
  for (const { index, segment } of graphemes.segment(text)) {
    const end = index + segment.length
    if (end > budget) break
    prefix = end
  }
  return prefix
}

export class StreamPacer {
  private pending: PacedText[] = [] // adjacent chunks of a stream merged
  private backlog = 0 // UTF-16 code units in `pending`
  private lastArrival: number | undefined
  private gap: number | undefined // expected time between arrivals, once one was seen
  private lastAccrual: number | undefined
  private rate = 0 // code units per millisecond
  private credit = 0 // code units due but not taken yet

  push(chunk: PacedText, now: number): void {
    this.accrue(now)
    this.merge(chunk)
    this.backlog += chunk.text.length

    // Only the gaps between network reads say when the next text is due. Chunks that came
    // in the same read would make it look like text arrives continuously.
    const sameRead = this.lastArrival !== undefined && now - this.lastArrival < BURST_WINDOW_MS
    if (!sameRead) {
      if (this.lastArrival !== undefined) {
        const gap = Math.min(MAX_LAG_MS, Math.max(MIN_GAP_MS, now - this.lastArrival))
        this.gap = this.gap === undefined ? gap : this.gap + (gap - this.gap) * GAP_SMOOTHING
      }
      this.lastArrival = now
    }

    this.rate = this.backlog / (this.gap ?? MAX_LAG_MS)
  }

  // What is due by `now`, in arrival order, adjacent chunks of a stream merged.
  take(now: number): PacedText[] {
    this.accrue(now)

    const due: PacedText[] = []
    while (this.pending.length > 0) {
      const first = this.pending[0]!
      const length = graphemePrefix(first.text, Math.floor(this.credit))
      if (length === 0) break // the next grapheme is not due yet

      due.push({ stream: first.stream, text: first.text.slice(0, length) })
      first.text = first.text.slice(length)
      this.credit -= length
      this.backlog -= length
      if (first.text.length > 0) break // the rest of this chunk is not due yet
      this.pending.shift()
    }
    return due
  }

  takeAll(): PacedText[] {
    const all = this.pending
    this.pending = []
    this.backlog = 0
    this.credit = 0
    return all
  }

  isEmpty(): boolean {
    return this.pending.length === 0
  }

  hasPending(stream: string): boolean {
    return this.pending.some((chunk) => chunk.stream === stream)
  }

  // Drops what waits for `stream`, e.g. when its text was rewritten rather than extended.
  discard(stream: string): void {
    const kept = this.pending.filter((chunk) => chunk.stream !== stream)
    this.backlog = kept.reduce((sum, chunk) => sum + chunk.text.length, 0)
    this.credit = Math.min(this.credit, this.backlog)
    this.pending = kept
  }

  private merge(chunk: PacedText): void {
    const last = this.pending.at(-1)
    if (last?.stream === chunk.stream) {
      last.text += chunk.text
      return
    }
    this.pending.push({ ...chunk })
  }

  // Credit grows at the current rate. It never exceeds the backlog: credit banked while
  // nothing waits would let the next arrival pop in at once.
  private accrue(now: number): void {
    const elapsed = this.lastAccrual === undefined ? 0 : now - this.lastAccrual
    this.lastAccrual = now
    this.credit = Math.min(this.credit + this.rate * elapsed, this.backlog)
  }
}
