// Starts the app: connects, then opens the requested session if given.

import { translate } from '../i18n/index.ts'
import { directBackendFromEnvironment } from '../net/backend.ts'
import { keys, sentKeyInClear, takeKeyFromAddress } from '../net/keystore.ts'
import { StartupError, browserEnvironment, resolveStartup } from '../net/startup.ts'
import { directServerUrl } from '../net/transport.ts'
import { useConnectionStore } from './connection.ts'
import { useMcpStore } from './mcp.ts'
import { useNoticesStore } from './notices.ts'
import { useNotificationsStore } from './notifications.ts'
import { useWorkspaceStore } from './workspace.ts'

// The key in the address (#key=, ?key=, ?token=) is taken into the key store and removed from the
// address bar. A key in the query went to the server as well; over plain http to another machine
// that was in clear text, which the user is told.
function takeKeyFromLocation(): void {
  if (directBackendFromEnvironment({ search: location.search, directUrl: directServerUrl() })) return
  const found = takeKeyFromAddress(new URL(location.href))
  if (found.key === undefined) return
  keys.setKey(found.key)
  history.replaceState(null, '', found.cleaned)
  if (found.inQuery && sentKeyInClear(found.cleaned)) {
    useNoticesStore().push('warn', translate('key.sentInClear'))
  }
}

export async function boot(): Promise<StartupError | undefined> {
  const connection = useConnectionStore()
  const workspace = useWorkspaceStore()
  const notices = useNoticesStore()
  const mcp = useMcpStore()

  let startup
  takeKeyFromLocation()
  try {
    startup = resolveStartup(browserEnvironment())
  } catch (error) {
    if (error instanceof StartupError) return error
    throw error
  }

  // A backend restarted after "too old" is the first usable connection, though not the first one.
  let loaded = false
  connection.onBeforeSwitch(() => {
    loaded = false
    workspace.resetForBackendSwitch()
    mcp.reset()
  })
  connection.onOpen((isReconnect) => {
    if (connection.backendTooOld) return
    const wasLoaded = loaded
    loaded = true
    workspace.onConnected(isReconnect && wasLoaded, startup).catch((error) => notices.pushError(error))
  })
  connection.onFrame((frame) => {
    if (frame.kind === 'toast') {
      notices.push(frame.level, frame.text)
      return
    }
    if (frame.kind === 'config_changed') {
      const disposition = connection.noteConfigChanged()
      if (disposition === 'external') {
        for (const warning of frame.warnings) notices.push('warn', warning)
        connection.loadConfig(workspace.effectiveCwd, { silentWarnings: true }).catch(notices.pushError)
        mcp.refresh().catch(notices.pushError)
      }
      return
    }
    workspace.onFrame(frame)
  })
  useNotificationsStore().start()
  connection.start(startup.url, startup.cwd, { direct: startup.direct, token: startup.token })
  return undefined
}
