// The connection to the backend: state, handshake result and the typed API.

import { defineStore } from 'pinia'
import { computed, onScopeDispose, ref, shallowRef } from 'vue'
import { RefApi } from '../net/api.ts'
import {
  clearSavedBackend,
  maskToken,
  readSavedBackend,
  resolveBackend,
  subscribeBackendStorage,
  validateBackendUrl,
  writeSavedBackend,
} from '../net/backend.ts'
import type { BackendConfig, BackendSource } from '../net/backend.ts'
import { channelTransport } from '../net/channel.ts'
import type { UpstreamTarget } from '../net/channel.ts'
import { RefConnection, protocolOf } from '../net/connection.ts'
import type { ConnectionState, EventFrame, HelloResult } from '../net/connection.ts'
import { keys } from '../net/keystore.ts'
import { directServerUrl, webSocket, webSocketTransport } from '../net/transport.ts'
import type { ServerInfo } from '../net/transport.ts'
import { PROTOCOL_VERSION } from '../proto/generated'
import type { ConfigFilesInfo, ServerConfigInfo } from '../proto/generated'
import { useNoticesStore } from './notices.ts'
import { useSettingsStore } from './settings.ts'

const CLIENT_NAME = 'ref-web'

export type SwitchBackendAction =
  | { mode: 'temporary'; url: string; token?: string }
  | { mode: 'saveDefault'; url: string; token?: string }
  | { mode: 'restoreDefault' }

export interface StartOptions {
  direct?: boolean
  token?: string
}

export const useConnectionStore = defineStore('connection', () => {
  const notices = useNoticesStore()
  const settings = useSettingsStore()
  const state = ref<ConnectionState>('connecting')
  const detail = ref('')
  const hello = shallowRef<HelloResult>()
  const config = shallowRef<ServerConfigInfo>()
  const serverInfo = shallowRef<ServerInfo>()
  const protocol = computed(() => protocolOf(hello.value))
  const backendTooOld = computed(() => hello.value !== undefined && protocol.value < PROTOCOL_VERSION)
  const fatal = ref<string>() // i18n key of an error that stops the app before it connects
  const keyRequired = ref(false) // ref-web needs a key we do not have: the key-entry page shows
  const reconnected = ref(0) // counts the times the connection came back after a loss

  const temporaryBackend = shallowRef<BackendConfig>()
  const savedBackend = shallowRef<BackendConfig | undefined>(readSavedBackend())
  const frameListeners = new Set<(frame: EventFrame) => void>()
  const openListeners = new Set<(isReconnect: boolean) => void>()
  const beforeSwitchListeners = new Set<() => void>()
  let connection: RefConnection | undefined
  let api: RefApi | undefined
  let opened = false

  let reloadingCount = 0
  let mutatingCount = 0
  let sawConfigChangedDuringMutation = false
  let configSerial = 0
  const address = shallowRef<{ url: string; cwd: string | undefined; direct?: boolean; token?: string }>()
  let keyOutcome: ((accepted: boolean) => void) | undefined

  const resolved = computed(() => {
    const addr = address.value
    return resolveBackend({
      directUrl: addr?.direct ? addr.url : directServerUrl(),
      temporary: temporaryBackend.value,
      saved: savedBackend.value,
      defaultUrl: addr?.url ?? '',
    })
  })
  const backendSource = computed<BackendSource>(() => resolved.value.source)
  const activeBackendUrl = computed(() => resolved.value.upstreamUrl ?? resolved.value.url)
  const activeBackendToken = computed(() => resolved.value.token ?? address.value?.token)
  const maskedBackendToken = computed(() => maskToken(activeBackendToken.value))
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
  function start(url: string, cwd?: string, options: StartOptions = {}): void {
    address.value = { url, cwd, direct: options.direct, token: options.token }
    const target = resolved.value

    if (!target.direct && !keys.hasKey()) {
      keys.required = true
      keyRequired.value = true
      return
    }

    const upstreamTarget: UpstreamTarget | undefined =
      !target.direct && target.upstreamUrl
        ? { mode: 'explicit', url: target.upstreamUrl, token: target.token }
        : undefined
    const token = target.token ?? options.token

    const created = new RefConnection({
      connect: target.direct
        ? webSocketTransport(target.url)
        : channelTransport(webSocket(target.url), keys, undefined, upstreamTarget),
      client: CLIENT_NAME,
      cwd,
      token,
      notifyKinds: () => settings.notifyKinds,
      giveUp: giveUpWithoutKey,
    })
    connection = created
    api = new RefApi(created)

    created.onState((next, why) => {
      if (created !== connection) return
      state.value = next
      detail.value = why
      if (next !== 'open') return
      keyRequired.value = false
      settleKey(true)
      hello.value = created.hello
      serverInfo.value = created.transportInfo
      const isReconnect = opened
      opened = true
      if (isReconnect) reconnected.value++
      for (const listener of openListeners) listener(isReconnect)
    })
    created.onFrame((frame) => {
      if (created !== connection) return
      for (const listener of frameListeners) listener(frame)
    })
    created.onError((error) => {
      if (created !== connection) return
      notices.pushError(error)
    })
    created.open()
  }

  function resetConnectionState(): void {
    settleKey(false)
    connection?.close()
    connection = undefined
    api = undefined
    opened = false
    reloadingCount = 0
    mutatingCount = 0
    sawConfigChangedDuringMutation = false
    configSerial++
    config.value = undefined
    hello.value = undefined
    serverInfo.value = undefined
    fatal.value = undefined
    state.value = 'connecting'
    detail.value = ''
    for (const listener of beforeSwitchListeners) listener()
  }

  function switchBackend(action: SwitchBackendAction): void {
    if (action.mode === 'temporary') {
      const checked = validateBackendUrl(action.url)
      if (!checked.ok) throw new Error(checked.errorKey)
      const token = action.token?.trim() || undefined
      temporaryBackend.value = token ? { url: checked.url, token } : { url: checked.url }
    } else if (action.mode === 'saveDefault') {
      savedBackend.value = writeSavedBackend({ url: action.url, token: action.token })
      temporaryBackend.value = undefined
    } else {
      const hadOverride = temporaryBackend.value !== undefined || savedBackend.value !== undefined
      temporaryBackend.value = undefined
      clearSavedBackend()
      savedBackend.value = undefined
      if (!hadOverride) return
    }
    resetConnectionState()
    const addr = address.value
    if (!addr) return
    start(addr.url, addr.cwd, { direct: addr.direct, token: addr.token })
  }

  const stopStorageSync = subscribeBackendStorage((next) => {
    const previous = savedBackend.value
    savedBackend.value = next
    const same = previous?.url === next?.url && previous?.token === next?.token
    const addr = address.value
    if (same || !addr || temporaryBackend.value || backendSource.value === 'direct') return
    resetConnectionState()
    start(addr.url, addr.cwd, { direct: addr.direct, token: addr.token })
  })
  onScopeDispose(stopStorageSync)

  // The key-entry page: signs in with the typed key. True when ref-web accepted it.
  function submitKey(key: string): Promise<boolean> {
    return new Promise((resolve) => {
      const addr = address.value
      if (!addr) return resolve(false)
      keys.setKey(key)
      settleKey(false)
      keyOutcome = resolve
      connection?.close()
      start(addr.url, addr.cwd, { direct: addr.direct, token: addr.token })
    })
  }

  function pushWarnings(warnings: readonly string[]): void {
    for (const warning of warnings) notices.push('warn', warning)
  }

  async function loadConfig(cwd?: string, options?: { silentWarnings?: boolean }): Promise<ServerConfigInfo> {
    const serial = ++configSerial
    const loaded = await getApi().getConfig(cwd)
    if (serial === configSerial) {
      config.value = loaded
      if (!options?.silentWarnings) pushWarnings(loaded.warnings)
    }
    return loaded
  }

  async function reloadConfig(cwd?: string): Promise<{ config: ServerConfigInfo; warnings: string[] }> {
    const serial = ++configSerial
    reloadingCount++
    try {
      const result = await getApi().reloadConfig(cwd)
      if (serial === configSerial) {
        config.value = result.config
        pushWarnings(result.warnings)
      }
      return result
    } finally {
      reloadingCount--
    }
  }

  // Classifies an incoming `config_changed` broadcast so local reloads/mutations never toast twice.
  function noteConfigChanged(): 'ignored' | 'mutating' | 'external' {
    if (reloadingCount > 0) return 'ignored'
    if (mutatingCount > 0) {
      sawConfigChangedDuringMutation = true
      return 'mutating'
    }
    return 'external'
  }

  async function mutateConfigFile(
    cwd: string | undefined,
    touchesActive: boolean,
    action: (client: RefApi) => Promise<ConfigFilesInfo>,
  ): Promise<ConfigFilesInfo> {
    mutatingCount++
    try {
      const info = await action(getApi())
      pushWarnings(info.warnings)
      const shouldRefresh = (sawConfigChangedDuringMutation || touchesActive) && mutatingCount === 1
      if (shouldRefresh) {
        sawConfigChangedDuringMutation = false
        await loadConfig(cwd, { silentWarnings: true })
      }
      return info
    } finally {
      mutatingCount--
      if (mutatingCount === 0) sawConfigChangedDuringMutation = false
    }
  }

  function getConfigFiles(file?: string, cwd?: string): Promise<ConfigFilesInfo> {
    return getApi().getConfigFiles(file, cwd)
  }

  function saveConfigFile(file: string, content: string, cwd?: string): Promise<ConfigFilesInfo> {
    return mutateConfigFile(cwd, file === 'ref.json', (client) => client.saveConfigFile(file, content, cwd))
  }

  function createConfigFile(
    options: {
      label?: string
      cloneCurrent?: boolean
      content?: string
      cwd?: string
    } = {},
  ): Promise<ConfigFilesInfo> {
    return mutateConfigFile(options.cwd, true, (client) => client.createConfigFile(options))
  }

  function selectConfigFile(file: string, cwd?: string): Promise<ConfigFilesInfo> {
    return mutateConfigFile(cwd, true, (client) => client.selectConfigFile(file, cwd))
  }

  function deleteConfigFile(file: string, cwd?: string): Promise<ConfigFilesInfo> {
    return mutateConfigFile(cwd, file === 'ref.json', (client) => client.deleteConfigFile(file, cwd))
  }

  function listBinaries(query: string, limit?: number): Promise<string[]> {
    return getApi().listBinaries(query, limit)
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

  function onBeforeSwitch(listener: () => void): void {
    beforeSwitchListeners.add(listener)
  }

  function stop(): void {
    connection?.close()
  }

  return {
    state,
    detail,
    hello,
    serverInfo,
    protocol,
    config,
    backendTooOld,
    reconnected,
    backendSource,
    temporaryBackend,
    savedBackend,
    activeBackendUrl,
    activeBackendToken,
    maskedBackendToken,
    fatal,
    keyRequired,
    start,
    switchBackend,
    submitKey,
    stop,
    loadConfig,
    reloadConfig,
    noteConfigChanged,
    getConfigFiles,
    saveConfigFile,
    createConfigFile,
    selectConfigFile,
    deleteConfigFile,
    listBinaries,
    getApi,
    shutdownServer,
    onFrame,
    onOpen,
    onBeforeSwitch,
  }
})
