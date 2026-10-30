// The conversation of the session on screen.
//
//   events --> Transcript (model) --flush each frame--> blocks (shallowRef) --> components
//
// Only `blocks` is reactive and it is replaced as a whole, at most once per frame.

import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import type { Block } from '../model/blocks.ts'
import { Transcript } from '../model/transcript.ts'
import { useFrame } from '../motion/useFrame.ts'
import type { PermissionRequest, SessionEvent, SessionInfo } from '../proto/generated'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'

export const useConversationStore = defineStore('conversation', () => {
  const connection = useConnectionStore()
  const notices = useNoticesStore()

  const blocks = shallowRef<readonly Block[]>([])
  const info = shallowRef<SessionInfo>()
  const permissions = shallowRef<PermissionRequest[]>([])
  const loaded = ref(false)
  const generation = ref(0) // counts the times the history was loaded
  const busy = computed(() => info.value !== undefined && info.value.status.state !== 'idle')

  let transcript = new Transcript({ reportError: (message) => notices.push('error', message) })
  const flushing = ref(false) // something waits for the next frame

  useFrame(flushing, (_frameSeconds, now) => {
    if (transcript.flush(now)) blocks.value = transcript.blocks
    flushing.value = transcript.needsFlush
  })

  function showHistory(): void {
    transcript.flush(performance.now())
    blocks.value = transcript.blocks
    generation.value++
    publish()
  }

  function publish(): void {
    info.value = transcript.info
    permissions.value = [...transcript.permissions.values()]
    flushing.value = true
  }

  // Attaches to `sessionId` and shows its history; later events keep it up to date.
  async function open(sessionId: string): Promise<void> {
    const snapshot = await connection.getApi().attach(sessionId, true)
    transcript = new Transcript({ reportError: (message) => notices.push('error', message) })
    transcript.load(snapshot)
    loaded.value = true
    showHistory()
  }

  function applyEvent(sessionId: string, event: SessionEvent): void {
    if (!loaded.value || transcript.info?.id !== sessionId) return // not the session on screen
    if (event.type === 'error') notices.push('error', event.text)
    transcript.apply(event, performance.now())
    publish()
  }

  // The connection came back: the history may have moved on without us.
  async function resync(): Promise<void> {
    const sessionId = transcript.info?.id
    if (!sessionId) return
    transcript.load(await connection.getApi().attach(sessionId, true))
    showHistory()
  }

  async function send(text: string): Promise<void> {
    const sessionId = transcript.info?.id
    if (!sessionId) throw new Error('no session is open')
    await connection.getApi().prompt(sessionId, text)
  }

  async function stop(): Promise<void> {
    const sessionId = transcript.info?.id
    if (!sessionId) return
    await connection.getApi().abort(sessionId)
    transcript.settle() // what is on screen should match what the session has
    publish()
  }

  async function answerPermission(callId: string, allow: boolean, always: boolean): Promise<void> {
    const sessionId = transcript.info?.id
    if (!sessionId) return
    await connection.getApi().permissionReply(sessionId, callId, allow, always)
  }

  return {
    blocks,
    generation,
    info,
    permissions,
    loaded,
    busy,
    open,
    applyEvent,
    resync,
    send,
    stop,
    answerPermission,
  }
})
