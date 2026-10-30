// Starts the app: connects, then opens the requested session or a new one.

import { StartupError, browserEnvironment, resolveStartup } from '../net/startup.ts'
import type { Startup } from '../net/startup.ts'
import { useConnectionStore } from './connection.ts'
import { useConversationStore } from './conversation.ts'
import { useNoticesStore } from './notices.ts'

export async function boot(): Promise<StartupError | undefined> {
  const connection = useConnectionStore()
  const conversation = useConversationStore()
  const notices = useNoticesStore()

  let startup: Startup
  try {
    startup = await resolveStartup(browserEnvironment())
  } catch (error) {
    if (error instanceof StartupError) return error
    throw error
  }

  async function openSession(isReconnect: boolean): Promise<void> {
    if (connection.backendTooOld) return
    try {
      if (isReconnect) {
        await conversation.resync()
        return
      }
      await connection.loadConfig()
      const sessionId = startup.sessionId ?? (await connection.getApi().createSession(startup.cwd)).id
      await conversation.open(sessionId)
    } catch (error) {
      notices.pushError(error)
    }
  }

  connection.onOpen((isReconnect) => void openSession(isReconnect))
  connection.onFrame((frame) => {
    if (frame.kind === 'event') conversation.applyEvent(frame.session_id, frame.event)
    if (frame.kind === 'toast') notices.push(frame.level, frame.text)
  })
  connection.start(startup.url, startup.cwd)
  return undefined
}
