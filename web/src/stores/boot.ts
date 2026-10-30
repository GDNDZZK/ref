// Starts the app: connects, then opens the requested session or a new one.

import { StartupError, browserEnvironment, resolveStartup } from '../net/startup.ts'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'
import { useWorkspaceStore } from './workspace.ts'

export async function boot(): Promise<StartupError | undefined> {
  const connection = useConnectionStore()
  const workspace = useWorkspaceStore()
  const notices = useNoticesStore()

  let startup
  try {
    startup = await resolveStartup(browserEnvironment())
  } catch (error) {
    if (error instanceof StartupError) return error
    throw error
  }

  connection.onOpen((isReconnect) => {
    if (connection.backendTooOld) return
    workspace.onConnected(isReconnect, startup).catch((error) => notices.pushError(error))
  })
  connection.onFrame((frame) => {
    if (frame.kind === 'toast') notices.push(frame.level, frame.text)
    else workspace.onFrame(frame)
  })
  connection.start(startup.url, startup.cwd)
  return undefined
}
