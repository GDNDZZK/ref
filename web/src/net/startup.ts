// Where to connect and which directory a new session starts in.
//
//   via ref-web:   /ws on this host; cwd from ?dir= (ref-web's login redirect), else /api/info
//   dev, direct:   VITE_REF_URL (a ref-server); cwd from ?dir=, ?cwd= or VITE_REF_CWD, else an error

import { SESSION_PARAM, SPLIT_VIEW, VIEW_PARAM } from './location.ts'
import { UNAUTHORIZED } from './login.ts'
import { defaultServerUrl, directServerUrl } from './transport.ts'

const INFO_PATH = '/api/info'

export interface Startup {
  url: string
  cwd: string
  sessionId: string | undefined // ?session=<id> opens that session instead of a new one
  split: boolean // ?view=split starts in split view (the `-w` of the TUI)
}

export class StartupError extends Error {
  readonly messageKey: string

  constructor(messageKey: string, detail: string) {
    super(detail)
    this.messageKey = messageKey
  }
}

interface Environment {
  search: string
  directUrl: string | undefined
  directCwd: string | undefined
  fetchInfo(): Promise<{ cwd: string }>
}

export async function resolveStartup(
  environment: Environment,
  url: string = defaultServerUrl(),
): Promise<Startup> {
  const params = new URLSearchParams(environment.search)
  const sessionId = params.get(SESSION_PARAM) ?? undefined
  const split = params.get(VIEW_PARAM) === SPLIT_VIEW
  const dir = params.get('dir')
  if (dir) return { url, cwd: dir, sessionId, split }

  if (environment.directUrl) {
    const cwd = params.get('cwd') ?? environment.directCwd
    if (!cwd) throw new StartupError('connection.noDirectory', 'no ?dir=, ?cwd= or VITE_REF_CWD')
    return { url, cwd, sessionId, split }
  }

  const info = await environment.fetchInfo()
  return { url, cwd: info.cwd, sessionId, split }
}

export function browserEnvironment(): Environment {
  return {
    search: location.search,
    directUrl: directServerUrl(),
    directCwd: import.meta.env.VITE_REF_CWD as string | undefined,
    async fetchInfo() {
      const response = await fetch(INFO_PATH)
      if (response.status === UNAUTHORIZED)
        throw new StartupError('connection.notLoggedIn', `${INFO_PATH}: ${UNAUTHORIZED}`)
      if (!response.ok) throw new StartupError('connection.closed', `${INFO_PATH}: ${response.status}`)
      return (await response.json()) as { cwd: string }
    },
  }
}
