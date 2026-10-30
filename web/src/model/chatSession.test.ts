import { describe, expect, it, vi } from 'vitest'
import type { RefApi } from '../net/api.ts'
import { ChatSession } from './chatSession.ts'
import { assistantMessage, info, snapshot, userMessage } from './testMessages.ts'

const FRAME_WAIT_MS = 60 // the session shows changes on the next animation frame
const afterFrame = () => new Promise((resolve) => setTimeout(resolve, FRAME_WAIT_MS))

function setup(sessionInfo = info({ id: 'ses_1' })) {
  const api = {
    attach: vi.fn().mockResolvedValue(snapshot([userMessage('u1', 'hello')], { info: sessionInfo })),
    detach: vi.fn().mockResolvedValue(undefined),
  }
  const notify = vi.fn()
  const draftRestored = vi.fn()
  const session = new ChatSession(
    'ses_1',
    { api: () => api as unknown as RefApi, notify, draftRestored },
    sessionInfo,
  )
  return { api, notify, draftRestored, session }
}

describe('ChatSession', () => {
  it('knows what the session list says before it is attached', () => {
    const { session } = setup()
    expect(session.info?.name).toBe('test')
    expect(session.attachment).toBe('detached')
    expect(session.loaded).toBe(false)
  })

  it('shows the history after attaching and says how it is attached', async () => {
    const { api, session } = setup()
    await session.attach(true)
    expect(api.attach).toHaveBeenCalledWith('ses_1', true)
    expect(session.attachment).toBe('foreground')
    expect(session.blocks.map((block) => block.kind)).toEqual(['user', 'meta'])
    expect(session.generation).toBe(1)
  })

  it('attaches as a watcher for split view tiles', async () => {
    const { session } = setup()
    await session.attach(false)
    expect(session.attachment).toBe('watching')
  })

  it('keeps the history when it is detached', async () => {
    const { api, session } = setup()
    await session.attach(true)
    await session.detach()
    expect(api.detach).toHaveBeenCalledWith('ses_1')
    expect(session.attachment).toBe('detached')
    expect(session.blocks).toHaveLength(2)
  })

  it('ignores events that come before the snapshot', () => {
    const { session } = setup()
    session.applyEvent({ type: 'message', message: userMessage('u2', 'early') })
    expect(session.blocks).toEqual([])
  })

  it('takes the new status from the session list, loaded or not', async () => {
    const { session } = setup()
    const busy = info({ id: 'ses_1', status: { state: 'busy' } })
    session.setInfo(busy)
    expect(session.busy).toBe(true)
    await session.attach(true)
    session.setInfo(info({ id: 'ses_1' }))
    expect(session.busy).toBe(false)
  })

  it('keeps an error event as a block after the messages', async () => {
    const { session } = setup()
    await session.attach(true)
    session.applyEvent({ type: 'error', text: 'rate limited' })
    await afterFrame()
    expect(session.blocks.at(-1)).toMatchObject({ kind: 'error', text: 'rate limited' })
  })

  it('removes the error block with the next prompt', async () => {
    const { api, session } = setup()
    Object.assign(api, { prompt: vi.fn().mockResolvedValue({ queued: false, message_id: 'u2' }) })
    await session.attach(true)
    session.applyEvent({ type: 'error', text: 'boom' })
    await session.send('again')
    expect(session.blocks.some((block) => block.kind === 'error')).toBe(false)
  })

  it('does not repeat the error the last message already shows', async () => {
    const { api, session } = setup()
    api.attach.mockResolvedValue(
      snapshot([userMessage('u1', 'hi'), assistantMessage('a1', [], { time_end: 1, error: 'boom' })]),
    )
    await session.attach(true)
    session.applyEvent({ type: 'error', text: 'boom' })
    await afterFrame()
    expect(session.blocks.filter((block) => block.kind === 'error')).toHaveLength(1)
    session.applyEvent({ type: 'error', text: 'other' })
    await afterFrame()
    expect(session.blocks.at(-1)).toMatchObject({ kind: 'error', text: 'other' })
  })

  it('derives what the session is doing from its status', async () => {
    const { session } = setup()
    expect(session.activity).toBe('idle')
    session.setInfo(info({ status: { state: 'busy' }, busy_since: 5 }))
    expect(session.activity).toBe('thinking')
    expect(session.busySince).toBe(5)
    session.setInfo(info({ status: { state: 'retrying', attempt: 2, next_at: 9, error: 'overloaded' } }))
    expect(session.activity).toBe('retrying')
    expect(session.retry).toEqual({ attempt: 2, at: 9, error: 'overloaded' })
  })

  it('loads the page before the oldest message and shows it above', async () => {
    const { api, session } = setup()
    api.attach.mockResolvedValue(snapshot([userMessage('u2', 'new')], { more_before: true }))
    const messagesBefore = vi
      .fn()
      .mockResolvedValue({ messages: [userMessage('u1', 'old')], more_before: false })
    Object.assign(api, { messagesBefore })
    await session.attach(true)
    expect(session.moreBefore).toBe(true)
    await session.loadOlder()
    await afterFrame()
    expect(messagesBefore).toHaveBeenCalledWith('ses_1', 'u2')
    expect(session.moreBefore).toBe(false)
    expect(session.blocks.map((block) => block.key)).toEqual(['u1:user', 'u1:meta', 'u2:user', 'u2:meta'])
  })

  it('drops an older page that arrives after the history was replaced', async () => {
    const { api, session } = setup()
    api.attach.mockResolvedValue(snapshot([userMessage('u2', 'new')], { more_before: true }))
    let deliver: (page: unknown) => void = () => {}
    const messagesBefore = vi.fn().mockReturnValue(new Promise((resolve) => (deliver = resolve)))
    Object.assign(api, { messagesBefore })
    await session.attach(true)
    const loading = session.loadOlder()
    api.attach.mockResolvedValue(snapshot([userMessage('u9', 'other')], { more_before: false }))
    await session.attach(true)
    deliver({ messages: [userMessage('u1', 'old')], more_before: false })
    await loading
    await afterFrame()
    expect(session.blocks.map((block) => block.key)).toEqual(['u9:user', 'u9:meta'])
    expect(session.loadingOlder).toBe(false)
  })

  it('keeps the error of a failed page load and tries again', async () => {
    const { api, session } = setup()
    api.attach.mockResolvedValue(snapshot([userMessage('u2', 'new')], { more_before: true }))
    const messagesBefore = vi.fn().mockRejectedValue(new Error('offline'))
    Object.assign(api, { messagesBefore })
    await session.attach(true)
    await session.loadOlder()
    expect(session.olderError).toBe('offline')
    expect(session.loadingOlder).toBe(false)
    messagesBefore.mockResolvedValue({ messages: [], more_before: false })
    await session.loadOlder()
    expect(session.olderError).toBe('')
  })

  it('hands the prompt it takes back to the composer', async () => {
    const { api, draftRestored, session } = setup()
    const undo = vi.fn().mockResolvedValue({ text: 'fix it [Image 1]', images: [] })
    Object.assign(api, { undo })
    await session.undo('u1')
    expect(undo).toHaveBeenCalledWith('ses_1', 'u1')
    expect(draftRestored).toHaveBeenCalledWith('ses_1', 'fix it [Image 1]', [])
    await session.undoLast()
    expect(undo).toHaveBeenLastCalledWith('ses_1', undefined)
  })

  it('toggles the default folds', () => {
    const { session } = setup()
    session.toggleReasoning()
    session.toggleDetails()
    expect([session.thinkingExpanded, session.detailsExpanded]).toEqual([true, true])
  })
})
