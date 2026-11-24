// Where to connect and which directory a new session starts in.
//
//   via ref-web:   /ws on this host; cwd from ?dir=, else the one ref-web reports (sealed frame 0)
//   dev, direct:   VITE_REF_URL (a ref-server); cwd from ?dir=, ?cwd= or VITE_REF_CWD, else an error

import { directBackendFromEnvironment, SERVER_PARAM, validateBackendUrl } from './backend.ts'
import { SESSION_PARAM, SPLIT_VIEW, VIEW_PARAM } from './location.ts'
import { defaultServerUrl, directServerUrl } from './transport.ts'

export interface Startup {
  url: string
  cwd: string | undefined // undefined: ask ref-web, when the connection opens
  sessionId: string | undefined // ?session=<id> opens that session instead of a new one
  split: boolean // ?view=split starts in split view (the `-w` of the TUI)
  direct?: boolean
  token?: string
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
}

export function resolveStartup(environment: Environment, url: string = defaultServerUrl()): Startup {
  const params = new URLSearchParams(environment.search)
  const serverParam = params.get(SERVER_PARAM)
  if (serverParam !== null) {
    const checked = validateBackendUrl(serverParam)
    if (!checked.ok) {
      throw new StartupError('connection.invalidServer', `invalid ?server=${serverParam}`)
    }
  }
  const direct = directBackendFromEnvironment(environment)
  const resolvedUrl = direct ? direct.url : url
  const sessionId = params.get(SESSION_PARAM) ?? undefined
  const split = params.get(VIEW_PARAM) === SPLIT_VIEW
  const dir = params.get('dir')
  const extra = direct ? { direct: true, ...(direct.token ? { token: direct.token } : {}) } : {}
  if (dir) return { url: resolvedUrl, cwd: dir, sessionId, split, ...extra }

  if (direct) {
    const cwd = params.get('cwd') ?? environment.directCwd
    if (!cwd) throw new StartupError('connection.noDirectory', 'no ?dir=, ?cwd= or VITE_REF_CWD')
    return { url: resolvedUrl, cwd, sessionId, split, ...extra }
  }
  return { url: resolvedUrl, cwd: undefined, sessionId, split }
}

export function browserEnvironment(): Environment {
  return {
    search: location.search,
    directUrl: directServerUrl(),
    directCwd: import.meta.env.VITE_REF_CWD as string | undefined,
  }
}
