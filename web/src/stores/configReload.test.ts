import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { TransportHandlers } from '../net/transport.ts'
import { PROTOCOL_VERSION, type ConfigFilesInfo, type ServerConfigInfo } from '../proto/generated'
import { boot } from './boot.ts'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'
import { useWorkspaceStore } from './workspace.ts'

let handlers: TransportHandlers | undefined
const sent: Record<string, unknown>[] = []

vi.mock('../net/transport.ts', () => ({
  defaultServerUrl: () => 'ws://direct/ws',
  directServerUrl: () => 'ws://direct/ws',
  webSocket: () => () => ({ send: () => {}, close: () => {} }),
  webSocketTransport: () => (h: TransportHandlers) => {
    handlers = h
    return {
      send: (raw: string) => {
        sent.push(JSON.parse(raw) as Record<string, unknown>)
      },
      close: () => {},
    }
  },
}))

const EMPTY_CONFIG: ServerConfigInfo = {
  agents: [],
  default_agent: 'build',
  models: [],
  commands: [],
  mcp: [],
  tui: null,
  warnings: [],
}

const SAMPLE_FILES: ConfigFilesInfo = {
  config_dir: '/home/user/.config/ref',
  files: [
    {
      file: 'ref.json',
      active: true,
      exists: true,
      label: 'Work',
      summary: '1 provider',
      modified_at: 1,
      size: 20,
    },
  ],
  selected_file: 'ref.json',
  raw_text: '{}\n',
  parsed: {},
  env_status: {},
  has_jsonc_override: false,
  project_overrides: [],
  warnings: [],
}

function respond(id: unknown, result: unknown): void {
  handlers!.message(JSON.stringify({ kind: 'response', id, ok: true, result, error: null }))
}

describe('config reload & config_changed coordination', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    sent.length = 0
    handlers = undefined
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
    vi.stubGlobal('location', new URL('http://localhost:3000/?cwd=/proj'))
    vi.stubGlobal('Notification', undefined)
  })

  it('deduplicates warnings when config_changed arrives during local reloadConfig or saveConfigFile, and reloads on external broadcasts', async () => {
    await boot()
    const connection = useConnectionStore()
    const notices = useNoticesStore()
    const workspace = useWorkspaceStore()

    // Complete handshake + initial startup requests
    handlers!.opened()
    respond(sent[0]!['id'], { server_version: '0.2.3', protocol: PROTOCOL_VERSION, cwd: '/proj' })
    await vi.waitFor(() => expect(connection.state).toBe('open'))

    for (const req of sent.slice(1)) {
      if (req['op'] === 'get_config') respond(req['id'], { config: EMPTY_CONFIG })
      if (req['op'] === 'list_sessions') respond(req['id'], { sessions: [] })
      if (req['op'] === 'list_history') respond(req['id'], { sessions: [] })
    }
    await vi.waitFor(() => expect(workspace.cwd).toBe('/proj'))
    sent.length = 0
    notices.notices.splice(0)

    // 1. reloadConfig: server broadcasts config_changed BEFORE responding to reload_config -> warning toasted ONCE!
    const reloading = connection.reloadConfig('/proj')
    const reloadReq = sent.find((r) => r['op'] === 'reload_config')!
    expect(reloadReq).toMatchObject({ op: 'reload_config', cwd: '/proj' })
    handlers!.message(JSON.stringify({ kind: 'config_changed', warnings: ['bad model'] }))
    respond(reloadReq['id'], {
      config: { ...EMPTY_CONFIG, warnings: ['bad model'] },
      warnings: ['bad model'],
    })
    await reloading
    expect(sent.filter((r) => r['op'] === 'get_config')).toHaveLength(0)
    expect(notices.notices.map((n) => [n.level, n.text, n.repeats])).toEqual([['warn', 'bad model', 0]])

    // 2. saveConfigFile('ref.json'): server broadcasts config_changed BEFORE responding -> warning toasted ONCE & get_config refreshed!
    sent.length = 0
    notices.notices.splice(0)
    const saving = connection.saveConfigFile('ref.json', '{}\n', '/proj')
    const saveReq = sent.find((r) => r['op'] === 'save_config_file')!
    handlers!.message(JSON.stringify({ kind: 'config_changed', warnings: ['save warning'] }))
    respond(saveReq['id'], { ...SAMPLE_FILES, warnings: ['save warning'] })
    await vi.waitFor(() => expect(sent.some((r) => r['op'] === 'get_config')).toBe(true))
    const getCfgReq = sent.find((r) => r['op'] === 'get_config')!
    expect(getCfgReq).toMatchObject({ op: 'get_config', cwd: '/proj' })
    respond(getCfgReq['id'], { config: { ...EMPTY_CONFIG, warnings: ['save warning'] } })
    await saving
    expect(notices.notices.map((n) => [n.level, n.text, n.repeats])).toEqual([['warn', 'save warning', 0]])

    // 3. External config_changed broadcast outside local mutations triggers get_config + mcp_status and toasts warning!
    sent.length = 0
    notices.notices.splice(0)
    handlers!.message(JSON.stringify({ kind: 'config_changed', warnings: ['external warning'] }))
    await vi.waitFor(() => expect(sent.some((r) => r['op'] === 'get_config')).toBe(true))
    expect(sent.some((r) => r['op'] === 'mcp_status')).toBe(true)
    expect(notices.notices.map((n) => [n.level, n.text])).toEqual([['warn', 'external warning']])
  })

  it('ignores stale out-of-order loadConfig and reloadConfig responses', async () => {
    await boot()
    const connection = useConnectionStore()
    const notices = useNoticesStore()
    const workspace = useWorkspaceStore()

    handlers!.opened()
    respond(sent[0]!['id'], { server_version: '0.2.3', protocol: PROTOCOL_VERSION, cwd: '/proj' })
    await vi.waitFor(() => expect(connection.state).toBe('open'))

    for (const req of sent.slice(1)) {
      if (req['op'] === 'get_config') respond(req['id'], { config: EMPTY_CONFIG })
      if (req['op'] === 'list_sessions') respond(req['id'], { sessions: [] })
      if (req['op'] === 'list_history') respond(req['id'], { sessions: [] })
    }
    await vi.waitFor(() => expect(workspace.cwd).toBe('/proj'))
    sent.length = 0
    notices.notices.splice(0)

    const slowOld = connection.loadConfig('/dir-a')
    const oldReq = sent.find((r) => r['op'] === 'get_config' && r['cwd'] === '/dir-a')!
    const fastNew = connection.loadConfig('/dir-b', { silentWarnings: true })
    const newReq = sent.find((r) => r['op'] === 'get_config' && r['cwd'] === '/dir-b')!

    const newConfig: ServerConfigInfo = { ...EMPTY_CONFIG, default_agent: 'plan', warnings: ['new warn'] }
    const oldConfig: ServerConfigInfo = { ...EMPTY_CONFIG, default_agent: 'build', warnings: ['stale warn'] }

    respond(newReq['id'], { config: newConfig })
    expect(await fastNew).toEqual(newConfig)
    expect(connection.config).toEqual(newConfig)

    respond(oldReq['id'], { config: oldConfig })
    expect(await slowOld).toEqual(oldConfig)
    expect(connection.config).toEqual(newConfig)
    expect(notices.notices).toEqual([])

    sent.length = 0
    const slowReload = connection.reloadConfig('/dir-a')
    const reloadReq = sent.find((r) => r['op'] === 'reload_config' && r['cwd'] === '/dir-a')!
    const fastLoad = connection.loadConfig('/dir-c', { silentWarnings: true })
    const loadReq = sent.find((r) => r['op'] === 'get_config' && r['cwd'] === '/dir-c')!

    const dirCConfig: ServerConfigInfo = { ...EMPTY_CONFIG, default_agent: 'review' }
    respond(loadReq['id'], { config: dirCConfig })
    expect(await fastLoad).toEqual(dirCConfig)
    expect(connection.config).toEqual(dirCConfig)

    const staleReloadResult = { config: oldConfig, warnings: ['stale reload warn'] }
    respond(reloadReq['id'], staleReloadResult)
    expect(await slowReload).toEqual(staleReloadResult)
    expect(connection.config).toEqual(dirCConfig)
    expect(notices.notices).toEqual([])
  })
})
