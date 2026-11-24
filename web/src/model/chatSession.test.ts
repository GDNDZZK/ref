import { describe, expect, it, vi } from 'vitest'
import type { RefApi } from '../net/api.ts'
import { RefError } from '../net/connection.ts'
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

  it('answers a permission request once, however fast the keys come', async () => {
    const { api, session } = setup()
    const request = {
      session_id: 'ses_1',
      message_id: 'a1',
      call_id: 'c1',
      tool: 'bash',
      purpose: '',
      input: {},
      time: 1,
    }
    api.attach.mockResolvedValue(snapshot([userMessage('u1', 'hi')], { pending_permissions: [request] }))
    const permissionReply = vi.fn().mockResolvedValue(undefined)
    Object.assign(api, { permissionReply })
    await session.attach(true)
    const first = session.answerPermission('c1', true, false)
    expect(session.pendingPermission).toBeUndefined()
    await session.answerPermission('c1', true, false)
    await first
    expect(permissionReply).toHaveBeenCalledTimes(1)
  })

  it('shows the request again when the answer could not be sent', async () => {
    const { api, session } = setup()
    const request = {
      session_id: 'ses_1',
      message_id: 'a1',
      call_id: 'c1',
      tool: 'bash',
      purpose: '',
      input: {},
      time: 1,
    }
    api.attach.mockResolvedValue(snapshot([userMessage('u1', 'hi')], { pending_permissions: [request] }))
    Object.assign(api, { permissionReply: vi.fn().mockRejectedValue(new Error('offline')) })
    await session.attach(true)
    await expect(session.answerPermission('c1', true, false)).rejects.toThrow('offline')
    expect(session.pendingPermission?.call_id).toBe('c1')
  })

  it('ignores a permission already answered elsewhere (coded and text fallback)', async () => {
    const request = {
      session_id: 'ses_1',
      message_id: 'a1',
      call_id: 'c1',
      tool: 'bash',
      purpose: '',
      input: {},
      time: 1,
    }
    for (const error of [
      new RefError('already answered elsewhere', 'no_pending_permission'),
      new Error('No pending permission request with that id.'),
    ]) {
      const { api, session } = setup()
      api.attach.mockResolvedValue(snapshot([userMessage('u1', 'hi')], { pending_permissions: [request] }))
      Object.assign(api, { permissionReply: vi.fn().mockRejectedValue(error) })
      await session.attach(true)
      await expect(session.answerPermission('c1', true, false)).resolves.toBeUndefined()
      expect(session.pendingPermission).toBeUndefined()
    }
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

  it('asks for a picture once, and again after the session was detached', async () => {
    const { api, session } = setup()
    const getImage = vi.fn().mockResolvedValue({ mime: 'image/png', data: 'AAAA' })
    Object.assign(api, { getImage })
    expect(await session.imageUrl('img_1')).toBe('data:image/png;base64,AAAA')
    await session.imageUrl('img_1')
    expect(getImage).toHaveBeenCalledTimes(1)
    await session.detach()
    await session.imageUrl('img_1')
    expect(getImage).toHaveBeenCalledTimes(2)
  })

  it('toggles the default folds', () => {
    const { session } = setup()
    session.toggleReasoning()
    session.toggleDetails()
    expect([session.thinkingExpanded, session.detailsExpanded]).toEqual([true, true])
  })
  it('debounces range requests by 80ms and drops stale range responses', async () => {
    vi.useFakeTimers()
    try {
      const { api, session } = setup()
      const metas = Array.from({ length: 4 }, (_, seq) => ({
        seq,
        id: `m${seq}`,
        role: 'user' as const,
        source_kind: 'human',
        has_error: false,
        parts: [],
      }))
      api.attach.mockResolvedValue(
        snapshot([userMessage('m4', 'latest')], {
          more_before: true,
          total_messages: 5,
          start_seq: 4,
          layout_metas: metas,
        }),
      )
      let resolveSlow!: (v: unknown) => void
      const messagesRange = vi.fn().mockImplementationOnce(
        () => new Promise((resolve) => { resolveSlow = resolve }),
      )
      Object.assign(api, { messagesRange })
      await session.attach(true)
      expect(session.slots).toHaveLength(5)

      session.requestRange(0, 2)
      session.requestRange(1, 3)
      expect(messagesRange).not.toHaveBeenCalled()
      vi.advanceTimersByTime(80)
      expect(messagesRange).toHaveBeenCalledTimes(1)

      // Re-attach bumps generation -> stale slow response is dropped.
      api.attach.mockResolvedValue(snapshot([userMessage('m9', 'fresh')]))
      await session.attach(true)
      resolveSlow({ messages: [userMessage('m0', 'stale')], more_before: false, start_seq: 0, end_seq: 1 })
      await Promise.resolve()
      await Promise.resolve()
      expect(session.slots.map((s) => s.id)).toEqual(['m9'])
    } finally {
      vi.useRealTimers()
    }
  })

  it('evicts outside window, drops evicted image caches, evicts at bottom, and trims on detach', async () => {
    const { api, session } = setup()
    const messages = Array.from({ length: 200 }, (_, i) => ({
      ...userMessage(`m${i}`, `text ${i}`),
      parts: [
        { type: 'text' as const, text: `text ${i}`, time: 0 },
        { type: 'image' as const, mime: 'image/png', data: '', id: `img_${i}`, size: 10 },
      ],
    }))
    api.attach.mockResolvedValue(snapshot(messages))
    const getImage = vi.fn().mockImplementation((_sid: string, imgId: string) =>
      Promise.resolve({ mime: 'image/png', data: imgId }),
    )
    Object.assign(api, { getImage })
    await session.attach(true)
    await session.imageUrl('img_0')
    await session.imageUrl('img_199')
    expect(getImage).toHaveBeenCalledTimes(2)

    // Evict around [180, 199] -> keeps [30..199], evicts 0..29 (including img_0).
    session.evictAround(180, 199)
    expect(session.residentCount).toBe(170)
    await session.imageUrl('img_199')
    expect(getImage).toHaveBeenCalledTimes(2) // still cached
    await session.imageUrl('img_0')
    expect(getImage).toHaveBeenCalledTimes(3) // evicted -> re-fetched

    // Evict at bottom keeps newest 100.
    session.evictAtBottom()
    expect(session.residentCount).toBe(100)

    // Detach evicts all but newest 100 and clears image cache.
    await session.detach()
    expect(session.residentCount).toBe(100)
    await session.imageUrl('img_199')
    expect(getImage).toHaveBeenCalledTimes(4)
  })
})
