// What is typed in one composer: text, shell mode and the attached images.
//
// An attached image is named by a token in the text ("[Image 1]"), as in the TUI. The model
// can refer to it by the token, and deleting the token from the text drops the image.

import type { ImageData, NamedImage } from '../proto/generated'

export interface Attachment {
  token: string
  image: ImageData
  name: string | undefined
}

export interface Draft {
  text: string
  shell: boolean // `!` mode: the text is a shell command
  attachments: Attachment[]
  imageSeq: number // number of the last token handed out
}

export interface Submission {
  text: string
  images: ImageData[]
}

const IMAGE_TOKEN = /\[Image (\d+)\]/g
const PARAGRAPH_BREAK = '\n\n'

export function emptyDraft(): Draft {
  return { text: '', shell: false, attachments: [], imageSeq: 0 }
}

export function isDraftEmpty(draft: Draft): boolean {
  return draft.text.trim() === '' && draft.attachments.length === 0
}

// Inserts the token for `image` at `cursor` (space-padded on the left unless the text before it
// already ends in a space or line break) and returns the cursor after it.
export function attachImage(
  draft: Draft,
  image: ImageData,
  name: string | undefined,
  cursor: number = draft.text.length,
): number {
  draft.imageSeq++
  const token = `[Image ${draft.imageSeq}]`
  draft.attachments.push({ token, image, name })

  const before = draft.text.slice(0, cursor)
  const needsPad = before !== '' && !/[ \n]$/.test(before)
  const inserted = `${needsPad ? ' ' : ''}${token} `
  draft.text = before + inserted + draft.text.slice(cursor)
  return before.length + inserted.length
}

// Drops the attachments whose token was deleted from the text.
export function dropOrphanAttachments(draft: Draft): void {
  draft.attachments = draft.attachments.filter((attachment) => draft.text.includes(attachment.token))
  if (draft.attachments.length === 0) draft.imageSeq = 0
}

function tokenNumbers(text: string): number[] {
  return [...text.matchAll(IMAGE_TOKEN)].map((match) => Number(match[1]))
}

// Puts a prompt that was sent before back, after a blank line if the box is not empty.
// The old tokens are bound to `images` in number order and renumbered after the draft's own;
// images without a token get one at the end. Returns the cursor, at the end of the text.
export function restoreInto(draft: Draft, text: string, images: readonly NamedImage[]): number {
  if (text.trim() === '' && images.length === 0) return draft.text.length

  const numbers = [...new Set(tokenNumbers(text))].sort((a, b) => a - b)
  const bound = numbers.slice(0, images.length)
  const rest = images.slice(bound.length)

  // Two passes through placeholders, so a renumbered token is never renumbered again.
  let restored = text
  bound.forEach((number, index) => {
    restored = restored.replaceAll(`[Image ${number}]`, `\u0000${index}\u0000`)
  })
  bound.forEach((_, index) => {
    draft.imageSeq++
    const token = `[Image ${draft.imageSeq}]`
    restored = restored.replaceAll(`\u0000${index}\u0000`, token)
    draft.attachments.push(attachmentOf(token, images[index]!))
  })

  const keep = draft.text.trimEnd()
  draft.text = keep === '' ? restored : keep + PARAGRAPH_BREAK + restored
  let cursor = draft.text.length
  for (const image of rest) cursor = attachImage(draft, image, image.name ?? undefined, cursor)
  return cursor
}

function attachmentOf(token: string, image: NamedImage): Attachment {
  const { name, ...data } = image
  return { token, image: data, name: name ?? undefined }
}

// The text and images to send: the attached images whose token is still in the text, in the
// order they were attached (which is the order of their numbers).
export function submissionOf(draft: Draft): Submission {
  const images = draft.attachments
    .filter((attachment) => draft.text.includes(attachment.token))
    .map((attachment) => attachment.image)
  return { text: draft.text, images }
}

export function clearDraft(draft: Draft): void {
  draft.text = ''
  draft.shell = false
  draft.attachments = []
  draft.imageSeq = 0
}
