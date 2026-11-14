import { describe, expect, it } from 'vitest'
import { fromHex, toHex } from './channelCrypto.ts'
import { KeyStore, STORAGE_KEY, sentKeyInClear, takeKeyFromAddress } from './keystore.ts'
import type { KeyStorage } from './keystore.ts'

const SALT = '00112233445566778899aabbccddeeff'
const OTHER_SALT = 'ffeeddccbbaa99887766554433221100'

// A "root" that shows what it was made from, without the cost of scrypt.
const fakeDerive = (key: string, salt: Uint8Array): Promise<Uint8Array> => {
  const root = new Uint8Array(32)
  root.set(new TextEncoder().encode(key).slice(0, 16))
  root[31] = salt[0]!
  return Promise.resolve(root)
}

function memoryStorage(): KeyStorage & { items: Map<string, string> } {
  const items = new Map<string, string>()
  return {
    items,
    getItem: (name) => items.get(name) ?? null,
    setItem: (name, value) => void items.set(name, value),
    removeItem: (name) => void items.delete(name),
  }
}

describe('KeyStore', () => {
  it('has nothing to sign in with at first, and asks for a key', async () => {
    const store = new KeyStore(memoryStorage(), fakeDerive)
    expect(store.hasKey()).toBe(false)
    expect(await store.rootFor(SALT)).toBeUndefined()
    expect(store.required).toBe(true)
  })

  it('stretches a given key with the salt and keeps the root, not the key', async () => {
    const storage = memoryStorage()
    const store = new KeyStore(storage, fakeDerive)
    store.setKey('my secret key')
    expect(store.hasKey()).toBe(true)
    const root = await store.rootFor(SALT)
    expect(root?.[31]).toBe(0x00)
    const stored = storage.items.get(STORAGE_KEY)!
    expect(JSON.parse(stored)).toEqual({ salt: SALT, root: toHex(root!) })
    expect(stored).not.toContain('my secret key')
  })

  it('reuses the stored root without deriving again, also in a new page', async () => {
    const storage = memoryStorage()
    const first = new KeyStore(storage, fakeDerive)
    first.setKey('a key')
    const root = await first.rootFor(SALT)
    const second = new KeyStore(storage, () => Promise.reject(new Error('must not derive')))
    expect(second.hasKey()).toBe(true)
    expect(await second.rootFor(SALT)).toEqual(root)
    expect(second.required).toBe(false)
  })

  it('treats another salt like no key', async () => {
    const storage = memoryStorage()
    const store = new KeyStore(storage, fakeDerive)
    store.setKey('a key')
    await store.rootFor(SALT)
    expect(await store.rootFor(OTHER_SALT)).toBeUndefined()
    expect(store.required).toBe(true)
  })

  it('a new key replaces the stored one', async () => {
    const storage = memoryStorage()
    const store = new KeyStore(storage, fakeDerive)
    store.setKey('old key')
    await store.rootFor(SALT)
    store.setKey('new key')
    expect(store.required).toBe(false)
    const root = await store.rootFor(SALT)
    expect(new TextDecoder().decode(root!.slice(0, 7))).toBe('new key')
  })

  it('forgets everything when ref-web rejects the key', async () => {
    const storage = memoryStorage()
    const store = new KeyStore(storage, fakeDerive)
    store.setKey('wrong')
    await store.rootFor(SALT)
    store.rejected()
    expect(storage.items.size).toBe(0)
    expect(store.hasKey()).toBe(false)
    expect(store.required).toBe(true)
  })

  it('ignores a damaged entry', async () => {
    for (const damaged of ['not json', '{"salt":1}', '{}', 'null']) {
      const storage = memoryStorage()
      storage.items.set(STORAGE_KEY, damaged)
      const store = new KeyStore(storage, fakeDerive)
      expect(store.hasKey(), damaged).toBe(false)
      expect(await store.rootFor(SALT), damaged).toBeUndefined()
    }
  })

  it('falls back to memory without usable storage', async () => {
    const throwing: KeyStorage = {
      getItem: () => {
        throw new Error('denied')
      },
      setItem: () => {
        throw new Error('denied')
      },
      removeItem: () => {
        throw new Error('denied')
      },
    }
    for (const storage of [throwing, undefined]) {
      const store = new KeyStore(storage, fakeDerive)
      store.setKey('a key')
      const root = await store.rootFor(SALT)
      expect(root).toBeDefined()
      expect(await store.rootFor(SALT)).toEqual(root) // from memory, no second derivation
      expect(fromHex(toHex(root!), 32)).toEqual(root)
    }
  })
})

describe('takeKeyFromAddress', () => {
  const take = (address: string) => takeKeyFromAddress(new URL(address))

  it('reads the fragment, which is never sent to the server', () => {
    const found = take('http://127.0.0.1:41873/?dir=/work#key=abc%20def')
    expect(found.key).toBe('abc def')
    expect(found.inQuery).toBe(false)
    expect(found.cleaned.href).toBe('http://127.0.0.1:41873/?dir=/work')
  })

  it('reads ?key= and the old ?token=, and keeps the other parameters', () => {
    for (const name of ['key', 'token']) {
      const found = take(`http://h:1/?session=ses_1&${name}=k3y&dir=%2Fa`)
      expect(found.key).toBe('k3y')
      expect(found.inQuery).toBe(true)
      expect(found.cleaned.searchParams.get('session')).toBe('ses_1')
      expect(found.cleaned.searchParams.get('dir')).toBe('/a')
      expect(found.cleaned.searchParams.has(name)).toBe(false)
    }
  })

  it('prefers the fragment and removes both', () => {
    const found = take('http://h:1/?key=fromquery#key=fromfragment')
    expect(found.key).toBe('fromfragment')
    expect(found.inQuery).toBe(false)
    expect(found.cleaned.href).toBe('http://h:1/')
  })

  it('leaves other fragment parameters alone and finds nothing in a plain address', () => {
    expect(take('http://h:1/#key=k&other=1').cleaned.hash).toBe('#other=1')
    const plain = take('http://h:1/?dir=/x#top')
    expect(plain.key).toBeUndefined()
    expect(plain.cleaned.hash).toBe('#top')
    expect(take('http://h:1/#key=').key).toBeUndefined()
  })

  it('knows when a query key travelled in clear text', () => {
    expect(sentKeyInClear(new URL('http://192.168.1.20:5000/'))).toBe(true)
    expect(sentKeyInClear(new URL('http://myhost:5000/'))).toBe(true)
    expect(sentKeyInClear(new URL('https://192.168.1.20:5000/'))).toBe(false)
    for (const local of ['http://127.0.0.1:1/', 'http://localhost:1/', 'http://[::1]:1/']) {
      expect(sentKeyInClear(new URL(local)), local).toBe(false)
    }
  })
})
