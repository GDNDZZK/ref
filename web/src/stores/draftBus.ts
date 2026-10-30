// How a prompt taken back (undo) finds its way into the input box: the part that undoes calls
// `emitDraftRestored`, the composer of that session listens. Neither knows the other.

import type { NamedImage } from '../proto/generated'

export type DraftRestoredListener = (sessionId: string, text: string, images: NamedImage[]) => void

const listeners = new Set<DraftRestoredListener>()

export function onDraftRestored(listener: DraftRestoredListener): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

// `images` carry their bytes (the response of Undo / RemoveQueued), not stubs.
export function emitDraftRestored(sessionId: string, text: string, images: NamedImage[]): void {
  for (const listener of listeners) listener(sessionId, text, images)
}
