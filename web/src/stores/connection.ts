// The connection to the backend: state, handshake result and the typed API.

import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import { RefApi } from '../net/api.ts'
import { RefConnection } from '../net/connection.ts'
import type { ConnectionState, EventFrame, HelloResult } from '../net/connection.ts'
import { webSocketTransport } from '../net/transport.ts'
import { PROTOCOL_VERSION } from '../proto/generated'
import type { ServerConfigInfo } from '../proto/generated'
import { useNoticesStore } from './notices.ts'
import { useSettingsStore } from './settings.ts'

const CLIENT_NAME = 'ref-web'
const PROTOCOL_WITHOUT_FIELD = 1 // servers older than protocol 2 do not report theirs

export const useConnectionStore = defineStore('connection', () => {
  const notices = useNoticesStore()
  const settings = useSettingsStore()
  const state = ref<ConnectionState>('connecting')
  const detail = ref('')
  const hello = shallowRef<HelloResult>()
  const config = shallowRef<ServerConfigInfo>()
  const backendTooOld = computed(
    () => hello.value !== undefined && (hello.value.protocol ?? PROTOCOL_WITHOUT_FIELD) < PROTOCOL_VERSION,
  )
  const fatal = ref<string>() // i18n key of an error that stops the app before it connects
  const reconnected = ref(0) // counts the times the connection came back after a loss

  const frameListeners = new Set<(frame: EventFrame) => void>()
  const openListeners = new Set<(isReconnect: boolean) => void>()
  let connection: RefConnection | undefined
  let api: RefApi | undefined
  let opened = false

  function start(url: string, cwd: string): void {
    const created = new RefConnection({
      connect: webSocketTransport(url),
      client: CLIENT_NAME,
      cwd,
      notifyKinds: () => settings.notifyKinds,
    })
    connection = created
    api = new RefApi(created)

    created.onState((next, why) => {
      state.value = next
      detail.value = why
      if (next !== 'open') return
      hello.value = created.hello
      const isReconnect = opened
      opened = true
      if (isReconnect) reconnected.value++
      for (const listener of openListeners) listener(isReconnect)
    })
    created.onFrame((frame) => {
      for (const listener of frameListeners) listener(frame)
    })
    created.onError((error) => notices.pushError(error))
    created.open()
  }

  async function loadConfig(): Promise<void> {
    config.value = await getApi().getConfig()
  }

  function getApi(): RefApi {
    if (!api) throw new Error('the connection has not been started')
    return api
  }

  function onFrame(listener: (frame: EventFrame) => void): void {
    frameListeners.add(listener)
  }

  // Called each time the connection opens, with true when it is a reconnect.
  function onOpen(listener: (isReconnect: boolean) => void): void {
    openListeners.add(listener)
  }

  function stop(): void {
    connection?.close()
  }

  return {
    state,
    detail,
    hello,
    config,
    backendTooOld,
    reconnected,
    fatal,
    start,
    stop,
    loadConfig,
    getApi,
    onFrame,
    onOpen,
  }
})
