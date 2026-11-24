import { describeError } from '../errorMessage.ts'
import { browserStorage, type KeyStorage } from './keystore.ts'
import { defaultServerUrl, directServerUrl } from './transport.ts'

export const BACKEND_STORAGE_KEY = 'ref.backend.v1'
export const SERVER_PARAM = 'server'
export const TOKEN_PARAM = 'token'

export interface BackendConfig {
  url: string
  token?: string
}

export type BackendSource = 'direct' | 'temporary' | 'saved' | 'auto'

export interface ResolvedBackend {
  source: BackendSource
  url: string
  upstreamUrl?: string
  token?: string
  direct: boolean
}

export type UrlValidation =
  { ok: true; url: string } | { ok: false; errorKey: 'dialog.backend.emptyUrl' | 'dialog.backend.invalidUrl' }

export function validateBackendUrl(raw: string): UrlValidation {
  const trimmed = raw.trim()
  if (trimmed === '') return { ok: false, errorKey: 'dialog.backend.emptyUrl' }
  try {
    const parsed = new URL(trimmed)
    const validScheme = parsed.protocol === 'ws:' || parsed.protocol === 'wss:'
    if (!validScheme || !parsed.host) return { ok: false, errorKey: 'dialog.backend.invalidUrl' }
    return { ok: true, url: parsed.toString() }
  } catch {
    return { ok: false, errorKey: 'dialog.backend.invalidUrl' }
  }
}

export function maskToken(token?: string): string {
  if (!token) return ''
  if (token.length <= 4) return '••••'
  return `••••${token.slice(-4)}`
}

export function parseSavedBackend(raw: string | null): BackendConfig | undefined {
  if (!raw) return undefined
  try {
    const parsed = JSON.parse(raw) as Partial<BackendConfig>
    if (typeof parsed.url !== 'string') return undefined
    const checked = validateBackendUrl(parsed.url)
    if (!checked.ok) return undefined
    const token = typeof parsed.token === 'string' && parsed.token !== '' ? parsed.token : undefined
    return token ? { url: checked.url, token } : { url: checked.url }
  } catch {
    return undefined
  }
}

export function readSavedBackend(
  storage: KeyStorage | undefined = browserStorage(),
): BackendConfig | undefined {
  try {
    return parseSavedBackend(storage?.getItem(BACKEND_STORAGE_KEY) ?? null)
  } catch {
    return undefined
  }
}

export function writeSavedBackend(
  config: BackendConfig,
  storage: KeyStorage | undefined = browserStorage(),
): BackendConfig {
  const checked = validateBackendUrl(config.url)
  if (!checked.ok) throw new Error(checked.errorKey)
  const token = config.token?.trim() || undefined
  const stored: BackendConfig = token ? { url: checked.url, token } : { url: checked.url }
  try {
    storage?.setItem(BACKEND_STORAGE_KEY, JSON.stringify(stored))
  } catch (error) {
    throw new Error(`Failed to save backend configuration: ${describeError(error)}`, { cause: error })
  }
  return stored
}

export function clearSavedBackend(storage: KeyStorage | undefined = browserStorage()): void {
  storage?.removeItem(BACKEND_STORAGE_KEY)
}

export function directBackendFromEnvironment(environment: {
  search: string
  directUrl?: string
}): BackendConfig | undefined {
  const params = new URLSearchParams(environment.search)
  const serverParam = params.get(SERVER_PARAM)
  const tokenParam = params.get(TOKEN_PARAM)?.trim() || undefined
  if (serverParam !== null) {
    const checked = validateBackendUrl(serverParam)
    if (checked.ok) {
      return tokenParam ? { url: checked.url, token: tokenParam } : { url: checked.url }
    }
  }
  if (environment.directUrl) {
    return tokenParam ? { url: environment.directUrl, token: tokenParam } : { url: environment.directUrl }
  }
  return undefined
}

export function resolveBackend(
  options: {
    search?: string
    directUrl?: string
    temporary?: BackendConfig
    saved?: BackendConfig
    defaultUrl?: string
  } = {},
): ResolvedBackend {
  const search = options.search ?? (typeof location !== 'undefined' ? location.search : '')
  const directUrl = options.directUrl !== undefined ? options.directUrl : directServerUrl()
  const direct = directBackendFromEnvironment({ search, directUrl })
  if (direct) {
    return {
      source: 'direct',
      url: direct.url,
      token: direct.token,
      direct: true,
    }
  }

  const fallbackUrl = options.defaultUrl ?? defaultServerUrl()
  if (options.temporary) {
    return {
      source: 'temporary',
      url: fallbackUrl,
      upstreamUrl: options.temporary.url,
      token: options.temporary.token,
      direct: false,
    }
  }
  const saved = 'saved' in options ? options.saved : readSavedBackend()
  if (saved) {
    return {
      source: 'saved',
      url: fallbackUrl,
      upstreamUrl: saved.url,
      token: saved.token,
      direct: false,
    }
  }
  return {
    source: 'auto',
    url: fallbackUrl,
    direct: false,
  }
}

export function subscribeBackendStorage(onChange: (saved: BackendConfig | undefined) => void): () => void {
  if (typeof window === 'undefined') return () => {}
  const handler = (event: StorageEvent): void => {
    if (event.key !== BACKEND_STORAGE_KEY && event.key !== null) return
    onChange(event.key === null ? undefined : parseSavedBackend(event.newValue))
  }
  window.addEventListener('storage', handler)
  return () => window.removeEventListener('storage', handler)
}
