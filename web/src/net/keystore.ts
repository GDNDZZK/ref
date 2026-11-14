// The key the page signs in to ref-web with.
//
// The page keeps `root` (the key stretched with scrypt and ref-web's salt), not the key, in
// localStorage under this origin; without localStorage it keeps it in memory and asks again
// after a reload. A key that comes in from the address or from the key-entry page waits in
// memory until the next connection has the salt to stretch it with.

import { deriveRoot, fromHex, toHex } from './channelCrypto.ts'

export const STORAGE_KEY = 'ref.web.key'
const FRAGMENT_PARAM = 'key'
const QUERY_PARAMS = ['key', 'token'] // `token` is what old bookmarks (/login?token=…) use

export interface KeyStorage {
  getItem(name: string): string | null
  setItem(name: string, value: string): void
  removeItem(name: string): void
}

interface Stored {
  salt: string
  root: string
}

export type Derive = (key: string, salt: Uint8Array) => Promise<Uint8Array>

export function browserStorage(): KeyStorage | undefined {
  try {
    return window.localStorage
  } catch {
    return undefined // some private modes refuse even the lookup
  }
}

export class KeyStore {
  // True once a connection found that it has no usable key: the key-entry page is due.
  required = false

  private pendingKey: string | undefined
  private memory: Stored | undefined
  private readonly storage: KeyStorage | undefined
  private readonly derive: Derive

  constructor(storage: KeyStorage | undefined, derive: Derive = deriveRoot) {
    this.storage = storage
    this.derive = derive
  }

  // Whether there is anything to sign in with.
  hasKey(): boolean {
    return this.pendingKey !== undefined || this.read() !== undefined
  }

  // A new key replaces the stored one.
  setKey(key: string): void {
    this.remove()
    this.pendingKey = key
    this.required = false
  }

  // The root for ref-web's salt (hex). Undefined when the user has to enter the key: nothing
  // is stored, or what is stored was made with another salt.
  async rootFor(saltHex: string): Promise<Uint8Array | undefined> {
    const salt = fromHex(saltHex, 16)
    const stored = this.read()
    if (stored?.salt === saltHex) {
      const root = fromHex(stored.root, 32)
      if (root) return root
    }
    const key = this.pendingKey
    if (key === undefined || !salt) {
      this.required = true
      return undefined
    }
    const root = await this.derive(key, salt)
    this.pendingKey = undefined
    this.write({ salt: saltHex, root: toHex(root) })
    return root
  }

  // ref-web did not accept what we sent: the key was changed. Forget everything.
  rejected(): void {
    this.pendingKey = undefined
    this.remove()
    this.required = true
  }

  private read(): Stored | undefined {
    let text: string | null
    try {
      text = this.storage?.getItem(STORAGE_KEY) ?? null
    } catch {
      return this.memory
    }
    if (text === null) return this.memory
    try {
      const parsed = JSON.parse(text) as Partial<Stored>
      if (typeof parsed.salt === 'string' && typeof parsed.root === 'string') {
        return { salt: parsed.salt, root: parsed.root }
      }
    } catch {
      // unreadable: treated as empty
    }
    return undefined
  }

  private write(stored: Stored): void {
    this.memory = stored
    try {
      this.storage?.setItem(STORAGE_KEY, JSON.stringify(stored))
    } catch {
      // full or refused: the memory copy serves until the page reloads
    }
  }

  private remove(): void {
    this.memory = undefined
    try {
      this.storage?.removeItem(STORAGE_KEY)
    } catch {
      // nothing stored there that we could reach
    }
  }
}

// The page's key.
export const keys = new KeyStore(browserStorage())

export interface AddressKey {
  key: string | undefined
  // The key came in the query string, which is sent to the server (the fragment never is).
  inQuery: boolean
  // The address without the key, for history.replaceState.
  cleaned: URL
}

// Reads `#key=K`, `?key=K` or `?token=K` and removes it from the address.
export function takeKeyFromAddress(url: URL): AddressKey {
  const cleaned = new URL(url)
  // Only the parts that hold a key are rewritten; the rest of the address stays as it is.
  const fragment = new URLSearchParams(cleaned.hash.slice(1))
  const fromFragment = fragment.get(FRAGMENT_PARAM)
  if (fragment.has(FRAGMENT_PARAM)) {
    fragment.delete(FRAGMENT_PARAM)
    const rest = fragment.toString()
    cleaned.hash = rest ? `#${rest}` : ''
  }

  let fromQuery: string | null = null
  for (const name of QUERY_PARAMS) {
    if (!cleaned.searchParams.has(name)) continue
    fromQuery ??= cleaned.searchParams.get(name)
    cleaned.searchParams.delete(name)
  }

  const key = fromFragment || fromQuery || undefined
  return { key, inQuery: !fromFragment && Boolean(fromQuery), cleaned }
}

function isLoopbackHost(hostname: string): boolean {
  return hostname === 'localhost' || hostname === '[::1]' || /^127\.\d+\.\d+\.\d+$/.test(hostname)
}

// A key in the query went over the network in clear text: plain http to another machine.
export function sentKeyInClear(url: URL): boolean {
  return url.protocol === 'http:' && !isLoopbackHost(url.hostname)
}
