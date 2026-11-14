// The connection to the backend: state, handshake result and the typed API.

import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import { RefApi } from '../net/api.ts'
import { RefConnection, protocolOf } from '../net/connection.ts'
import type { ConnectionState, EventFrame, HelloResult } from '../net/connection.ts'
import { channelTransport } from '../net/channel.ts'
import { keys } from '../net/keystore.ts'
import { directServerUrl, webSocket, webSocketTransport } from '../net/transport.ts'
import { PROTOCOL_VERSION } from '../proto/generated'
import type { ServerConfigInfo } from '../proto/generated'
import { useNoticesStore } from './notices.ts'
import { useSettingsStore } from './settings.ts'

const CLIENT_NAME = 'ref-web'

export const useConnectionStore = defineStore('connection', () => {
  const notices = useNoticesStore()
  const settings = useSettingsStore()
  const state = ref<ConnectionState>('connecting')
  const detail = ref('')
  const hello = shallowRef<HelloResult>()
  const config = shallowRef<ServerConfigInfo>()
  const protocol = computed(() => protocolOf(hello.value))
  const backendTooOld = computed(() => hello.value !== undefined && protocol.value < PROTOCOL_VERSION)
  const fatal = ref<string>() // i18n key of an error that stops the app before it connects
  const keyRequired = ref(false) // ref-web needs a key we do not have: the key-entry page shows
  const reconnected = ref(0) // counts the times the connection came back after a loss

  const frameListeners = new Set<(frame: EventFrame) => void>()
  const openListeners = new Set<(isReconnect: boolean) => void>()
  let connection: RefConnection | undefined
  let api: RefApi | undefined
  let opened = false

  let address: { url: string; cwd: string | undefined } | undefined
  let keyOutcome: ((accepted: boolean) => void) | undefined

  function settleKey(accepted: boolean): void {
    keyOutcome?.(accepted)
    keyOutcome = undefined
  }

  // A refused key is refused again: stop retrying and ask for a key.
  async function giveUpWithoutKey(): Promise<boolean> {
    if (!keys.required) return false
    keyRequired.value = true
    settleKey(false)
    return true
  }

  // `cwd`: the directory new sessions start in; ref-web's own is used when there is none.
  function start(url: string, cwd?: string): void {
    address = { url, cwd }
    // A dev build talks to a ref-server directly: no key, no channel.
    const direct = directServerUrl() !== undefined
    if (!direct && !keys.hasKey()) {
      keys.required = true
      keyRequired.value = true
      return
    }
    const created = new RefConnection({
      connect: direct ? webSocketTransport(url) : channelTransport(webSocket(url), keys),
      client: CLIENT_NAME,
      cwd,
      notifyKinds: () => settings.notifyKinds,
      giveUp: giveUpWithoutKey,
    })
    connection = created
    api = new RefApi(created)

    created.onState((next, why) => {
      state.value = next
      detail.value = why
      if (next !== 'open') return
      keyRequired.value = false
      settleKey(true)
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

  // The key-entry page: signs in with the typed key. True when ref-web accepted it.
  function submitKey(key: string): Promise<boolean> {
    return new Promise((resolve) => {
      if (!address) return resolve(false)
      keys.setKey(key)
      settleKey(false)
      keyOutcome = resolve
      connection?.close()
      start(address.url, address.cwd)
    })
  }

  async function loadConfig(): Promise<void> {
    config.value = await getApi().getConfig()
  }

  function getApi(): RefApi {
    if (!api) throw new Error('the connection has not been started')
    return api
  }

  // Stops the backend; the answer may never arrive, as the connection ends with it.
  function shutdownServer(): Promise<void> {
    return getApi().shutdown()
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
    protocol,
    config,
    backendTooOld,
    reconnected,
    fatal,
    keyRequired,
    start,
    submitKey,
    stop,
    loadConfig,
    getApi,
    shutdownServer,
    onFrame,
    onOpen,
  }
})
