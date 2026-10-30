import { describe, expect, it } from 'vitest'
import type { ImageData } from '../proto/generated'
import {
  attachImage,
  clearDraft,
  dropOrphanAttachments,
  emptyDraft,
  isDraftEmpty,
  restoreInto,
  submissionOf,
} from './draft.ts'

function image(data: string): ImageData {
  return { mime: 'image/png', data }
}

describe('attachImage', () => {
  it('inserts a token at the cursor and returns the cursor after it', () => {
    const draft = emptyDraft()
    draft.text = 'look at  please'
    const cursor = attachImage(draft, image('a'), 'a.png', 8)
    expect(draft.text).toBe('look at [Image 1]  please')
    expect(draft.text.slice(0, cursor)).toBe('look at [Image 1] ')
  })

  it('pads the token from the word before it', () => {
    const draft = emptyDraft()
    draft.text = 'see'
    attachImage(draft, image('a'), undefined)
    expect(draft.text).toBe('see [Image 1] ')
  })

  it('numbers the tokens one after the other', () => {
    const draft = emptyDraft()
    attachImage(draft, image('a'), undefined)
    attachImage(draft, image('b'), undefined)
    expect(draft.text).toBe('[Image 1] [Image 2] ')
    expect(draft.attachments.map((a) => a.token)).toEqual(['[Image 1]', '[Image 2]'])
  })
})

describe('dropOrphanAttachments', () => {
  it('forgets images whose token was deleted and restarts the numbering when none is left', () => {
    const draft = emptyDraft()
    attachImage(draft, image('a'), undefined)
    attachImage(draft, image('b'), undefined)
    draft.text = draft.text.replace('[Image 1] ', '')
    dropOrphanAttachments(draft)
    expect(draft.attachments.map((a) => a.token)).toEqual(['[Image 2]'])
    expect(draft.imageSeq).toBe(2)

    draft.text = ''
    dropOrphanAttachments(draft)
    expect(draft.attachments).toEqual([])
    expect(draft.imageSeq).toBe(0)
  })
})

describe('restoreInto', () => {
  it('fills an empty draft', () => {
    const draft = emptyDraft()
    restoreInto(draft, 'hello', [])
    expect(draft.text).toBe('hello')
  })

  it('appends after a blank line when something is typed', () => {
    const draft = emptyDraft()
    draft.text = 'typing  '
    const cursor = restoreInto(draft, 'back', [])
    expect(draft.text).toBe('typing\n\nback')
    expect(cursor).toBe(draft.text.length)
  })

  it('renumbers the restored tokens after the draft’s own', () => {
    const draft = emptyDraft()
    attachImage(draft, image('own'), undefined)
    restoreInto(draft, 'compare [Image 2] with [Image 1]', [
      { ...image('first'), name: 'first.png' },
      { ...image('second'), name: null },
    ])
    expect(draft.text).toBe('[Image 1]\n\ncompare [Image 3] with [Image 2]')
    expect(draft.attachments.map((a) => [a.token, a.image.data, a.name])).toEqual([
      ['[Image 1]', 'own', undefined],
      ['[Image 2]', 'first', 'first.png'],
      ['[Image 3]', 'second', undefined],
    ])
  })

  it('gives a token to an image the text does not mention', () => {
    const draft = emptyDraft()
    restoreInto(draft, 'plain', [image('x')])
    expect(draft.text).toBe('plain [Image 1] ')
    expect(draft.attachments).toHaveLength(1)
  })

  it('ignores a prompt with neither text nor images', () => {
    const draft = emptyDraft()
    restoreInto(draft, '  ', [])
    expect(draft.text).toBe('')
  })
})

describe('submissionOf', () => {
  it('sends the text and the images still named in it', () => {
    const draft = emptyDraft()
    attachImage(draft, image('a'), undefined)
    attachImage(draft, image('b'), undefined)
    draft.text = draft.text.replace('[Image 1] ', 'plain ')
    expect(submissionOf(draft)).toEqual({ text: 'plain [Image 2] ', images: [image('b')] })
  })
})

describe('draft state', () => {
  it('is empty with only whitespace and no images', () => {
    const draft = emptyDraft()
    draft.text = ' \n'
    expect(isDraftEmpty(draft)).toBe(true)
    attachImage(draft, image('a'), undefined)
    expect(isDraftEmpty(draft)).toBe(false)
  })

  it('clears everything', () => {
    const draft = emptyDraft()
    draft.shell = true
    attachImage(draft, image('a'), undefined)
    clearDraft(draft)
    expect(draft).toEqual(emptyDraft())
  })
})
