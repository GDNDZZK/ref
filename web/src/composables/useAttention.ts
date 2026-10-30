// The page's attention marker (title dot and alert favicon), shared by whoever sets the tab title.

import { documentAttention } from '../notify/attention.ts'
import type { Attention } from '../notify/attention.ts'

let attention: Attention | undefined

export function pageAttention(): Attention {
  attention ??= documentAttention(document)
  return attention
}

// Use this, not document.title, so the alert mark survives a title change.
export function setBaseTitle(title: string): void {
  pageAttention().setBaseTitle(title)
}
