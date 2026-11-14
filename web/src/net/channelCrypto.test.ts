// @vitest-environment node
/// <reference types="node" />
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  BadFrame,
  KIND_BINARY,
  KIND_TEXT,
  Opener,
  Sealer,
  authMac,
  channelKeys,
  deriveRoot,
  fromHex,
  toHex,
  utf8,
} from './channelCrypto.ts'
import type { Kind } from './channelCrypto.ts'

// The same vectors the Rust side checks (crates/web/src/channel_tests.rs).
const FIXTURE = fileURLToPath(new URL('../../../crates/web/fixtures/channel.json', import.meta.url))

interface Frame {
  counter: number
  kind: 'text' | 'binary'
  plain: string
  wire: string
}

const fixture = JSON.parse(readFileSync(FIXTURE, 'utf8')) as {
  key: string
  salt: string
  root: string
  k_auth: string
  sn: string
  cn: string
  mac: string
  k_c2s: string
  k_s2c: string
  c2s: Frame[]
  s2c: Frame[]
}

const bytes = (hex: string, length: number): Uint8Array => fromHex(hex, length)!
const kindOf = (frame: Frame): Kind => (frame.kind === 'text' ? KIND_TEXT : KIND_BINARY)

describe('derivation', () => {
  it('matches the shared vectors', async () => {
    const root = await deriveRoot(fixture.key, bytes(fixture.salt, 16))
    expect(toHex(root)).toBe(fixture.root)
    const [sn, cn] = [bytes(fixture.sn, 16), bytes(fixture.cn, 16)]
    expect(toHex(authMac(root, sn, cn))).toBe(fixture.mac)
    const keys = channelKeys(root, sn, cn)
    expect(toHex(keys.toServer)).toBe(fixture.k_c2s)
    expect(toHex(keys.toPage)).toBe(fixture.k_s2c)
  })

  it('depends on the key and on the salt', async () => {
    const salt = bytes(fixture.salt, 16)
    const root = await deriveRoot(fixture.key, salt)
    expect(toHex(await deriveRoot(`${fixture.key}!`, salt))).not.toBe(toHex(root))
    expect(toHex(await deriveRoot(fixture.key, new Uint8Array(16)))).not.toBe(toHex(root))
  })
})

describe('frames', () => {
  const keys = () => ({ toServer: bytes(fixture.k_c2s, 32), toPage: bytes(fixture.k_s2c, 32) })

  it('seal to the shared vectors, in both directions', () => {
    const { toServer, toPage } = keys()
    const toServerSealer = new Sealer(toServer)
    for (const frame of fixture.c2s) {
      expect(toHex(toServerSealer.seal(kindOf(frame), utf8(frame.plain)))).toBe(frame.wire)
    }
    const toPageSealer = new Sealer(toPage)
    for (const frame of fixture.s2c) {
      expect(toHex(toPageSealer.seal(kindOf(frame), utf8(frame.plain)))).toBe(frame.wire)
    }
  })

  it('open the shared vectors with their kind', () => {
    const opener = new Opener(keys().toPage)
    for (const frame of fixture.s2c) {
      const opened = opener.open(bytes(frame.wire, frame.wire.length / 2))
      expect(opened.kind).toBe(kindOf(frame))
      expect(new TextDecoder().decode(opened.plaintext)).toBe(frame.plain)
    }
  })

  const sealed = (count: number): Uint8Array[] => {
    const sealer = new Sealer(keys().toPage)
    return Array.from({ length: count }, (_, i) => sealer.seal(KIND_TEXT, utf8(`frame ${i}`)))
  }

  it('refuse a changed, truncated or foreign frame', () => {
    const [first] = sealed(1)
    const changed = first!.slice()
    changed[0]! ^= 1
    expect(() => new Opener(keys().toPage).open(changed)).toThrow(BadFrame)
    expect(() => new Opener(keys().toPage).open(first!.slice(0, -1))).toThrow(BadFrame)
    expect(() => new Opener(keys().toPage).open(new Uint8Array(4))).toThrow(BadFrame)
    expect(() => new Opener(keys().toServer).open(first!)).toThrow(BadFrame)
  })

  it('refuse repeated, dropped and reordered frames, and go on after a good one', () => {
    const [one, two, three] = sealed(3) as [Uint8Array, Uint8Array, Uint8Array]
    const opener = new Opener(keys().toPage)
    expect(() => opener.open(two)).toThrow(BadFrame) // reordered
    expect(opener.open(one).kind).toBe(KIND_TEXT)
    expect(() => opener.open(one)).toThrow(BadFrame) // repeated
    expect(() => opener.open(three)).toThrow(BadFrame) // dropped
    expect(opener.open(two).plaintext).toEqual(utf8('frame 1'))
    expect(opener.open(three).plaintext).toEqual(utf8('frame 2'))
  })

  it('count past 32 bits', () => {
    const sealer = new Sealer(keys().toPage)
    const counter = (sealer as unknown as { counter: { low: number } }).counter
    counter.low = 0xffffffff
    const last = sealer.seal(KIND_TEXT, utf8('a'))
    const next = sealer.seal(KIND_TEXT, utf8('b'))
    expect(toHex(last)).not.toBe(toHex(next))
    const opener = new Opener(keys().toPage)
    const openerCounter = (opener as unknown as { counter: { low: number } }).counter
    openerCounter.low = 0xffffffff
    expect(opener.open(last).plaintext).toEqual(utf8('a'))
    expect(opener.open(next).plaintext).toEqual(utf8('b'))
  })
})

describe('hex', () => {
  it('round trips and refuses bad input', () => {
    expect(toHex(Uint8Array.of(0, 15, 255))).toBe('000fff')
    expect(fromHex('000fFF', 3)).toEqual(Uint8Array.of(0, 15, 255))
    expect(fromHex('000f', 3)).toBeUndefined()
    expect(fromHex('zz', 1)).toBeUndefined()
  })
})
