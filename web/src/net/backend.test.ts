import { describe, expect, it, vi } from 'vitest'
import {
  BACKEND_STORAGE_KEY,
  clearSavedBackend,
  maskToken,
  readSavedBackend,
  resolveBackend,
  subscribeBackendStorage,
  validateBackendUrl,
  writeSavedBackend,
} from './backend.ts'

describe('backend', () => {
  it('validates ws:// and wss:// URLs and rejects invalid schemes', () => {
    expect(validateBackendUrl('')).toEqual({ ok: false, errorKey: 'dialog.backend.emptyUrl' })
    expect(validateBackendUrl('http://localhost:8080/ws')).toEqual({
      ok: false,
      errorKey: 'dialog.backend.invalidUrl',
    })
    expect(validateBackendUrl('ws://127.0.0.1:9000/ws')).toEqual({
      ok: true,
      url: 'ws://127.0.0.1:9000/ws',
    })
    expect(validateBackendUrl('wss://example.com/ws')).toEqual({
      ok: true,
      url: 'wss://example.com/ws',
    })
  })

  it('persists and clears ref.backend.v1 and masks tokens', () => {
    const store = new Map<string, string>()
    const storage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => {
        store.set(k, v)
      },
      removeItem: (k: string) => {
        store.delete(k)
      },
    }

    writeSavedBackend({ url: 'wss://remote.dev/ws', token: 'secret-token-1234' }, storage)
    expect(readSavedBackend(storage)).toEqual({
      url: 'wss://remote.dev/ws',
      token: 'secret-token-1234',
    })
    expect(JSON.parse(store.get(BACKEND_STORAGE_KEY)!)).toEqual({
      url: 'wss://remote.dev/ws',
      token: 'secret-token-1234',
    })
    expect(maskToken('secret-token-1234')).toBe('••••1234')
    expect(maskToken('ab')).toBe('••••')
    expect(maskToken()).toBe('')

    clearSavedBackend(storage)
    expect(readSavedBackend(storage)).toBeUndefined()
  })

  it('resolves backends in order: direct > temporary > saved > auto', () => {
    const base = {
      search: '',
      directUrl: undefined,
      defaultUrl: 'ws://localhost/ws',
      saved: { url: 'ws://saved/ws', token: 'tok-saved' },
      temporary: { url: 'ws://temp/ws', token: 'tok-temp' },
    }
    expect(resolveBackend({ ...base, search: '?server=wss://cli/ws&token=tok-cli' })).toEqual({
      source: 'direct',
      url: 'wss://cli/ws',
      token: 'tok-cli',
      direct: true,
    })
    expect(resolveBackend({ ...base, directUrl: 'ws://env/ws' })).toEqual({
      source: 'direct',
      url: 'ws://env/ws',
      token: undefined,
      direct: true,
    })
    expect(resolveBackend(base)).toEqual({
      source: 'temporary',
      url: 'ws://localhost/ws',
      upstreamUrl: 'ws://temp/ws',
      token: 'tok-temp',
      direct: false,
    })
    expect(resolveBackend({ ...base, temporary: undefined })).toEqual({
      source: 'saved',
      url: 'ws://localhost/ws',
      upstreamUrl: 'ws://saved/ws',
      token: 'tok-saved',
      direct: false,
    })
    expect(resolveBackend({ search: '', directUrl: undefined, defaultUrl: 'ws://localhost/ws' })).toEqual({
      source: 'auto',
      url: 'ws://localhost/ws',
      direct: false,
    })
  })

  it('syncs saved backend changes across tabs via storage events', () => {
    localStorage.clear()
    const onChange = vi.fn()
    const unsubscribe = subscribeBackendStorage(onChange)
    try {
      localStorage.setItem(BACKEND_STORAGE_KEY, JSON.stringify({ url: 'ws://peer:9000/ws' }))
      window.dispatchEvent(
        new StorageEvent('storage', {
          key: BACKEND_STORAGE_KEY,
          newValue: localStorage.getItem(BACKEND_STORAGE_KEY),
        }),
      )
      expect(onChange).toHaveBeenCalledWith({ url: 'ws://peer:9000/ws' })
    } finally {
      unsubscribe()
    }
  })
})
