// The cryptography of the channel to ref-web, the page's half of crates/web/src/channel.rs.
// Both sides are checked against crates/web/fixtures/channel.json.
//
//   root     = scrypt(key, salt)                      what the page stores, never the key
//   k_auth   = HKDF(root, "", "ref-web auth v1")
//   channel  = HKDF(root, sn ‖ cn, "ref-web channel v1", 64): client→server, server→client
//   frame    = XChaCha20-Poly1305(nonce = 16 zero bytes ‖ counter, aad = [kind]) ‖ tag

import { xchacha20poly1305 } from '@noble/ciphers/chacha.js'
import { hkdf } from '@noble/hashes/hkdf.js'
import { hmac } from '@noble/hashes/hmac.js'
import { scryptAsync } from '@noble/hashes/scrypt.js'
import { sha256 } from '@noble/hashes/sha2.js'

export const PROTOCOL_VERSION = 1
export const SALT_BYTES = 16
export const NONCE_BYTES = 16
const KEY_BYTES = 32
const TAG_BYTES = 16
const XNONCE_BYTES = 24
const COUNTER_BYTES = 8

// The cost is fixed on both sides and not sent: 2^15 blocks of 1 KiB = 32 MiB per guess.
const SCRYPT = { N: 2 ** 15, r: 8, p: 1, dkLen: KEY_BYTES }

const AUTH_LABEL = utf8('ref-web auth v1')
const CHANNEL_LABEL = utf8('ref-web channel v1')

// Text or binary message; authenticated with the frame.
export const KIND_TEXT = 0
export const KIND_BINARY = 1
export type Kind = typeof KIND_TEXT | typeof KIND_BINARY
const KINDS: Kind[] = [KIND_TEXT, KIND_BINARY]

// A frame that does not open: wrong key, changed, dropped, repeated or reordered.
export class BadFrame extends Error {}

export function utf8(text: string): Uint8Array {
  return new TextEncoder().encode(text)
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

// Undefined unless `text` is hex of exactly `length` bytes.
export function fromHex(text: string, length: number): Uint8Array | undefined {
  if (text.length !== length * 2 || !/^[0-9a-fA-F]*$/.test(text)) return undefined
  return Uint8Array.from({ length }, (_, i) => parseInt(text.slice(i * 2, i * 2 + 2), 16))
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0))
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.length
  }
  return out
}

// Takes a moment (up to a second on a phone); the work yields to the page now and then.
export async function deriveRoot(key: string, salt: Uint8Array): Promise<Uint8Array> {
  return scryptAsync(utf8(key), salt, SCRYPT)
}

function authKey(root: Uint8Array): Uint8Array {
  return hkdf(sha256, root, new Uint8Array(0), AUTH_LABEL, KEY_BYTES)
}

export function authMac(root: Uint8Array, sn: Uint8Array, cn: Uint8Array): Uint8Array {
  return hmac(sha256, authKey(root), concat(AUTH_LABEL, sn, cn))
}

export function channelKeys(
  root: Uint8Array,
  sn: Uint8Array,
  cn: Uint8Array,
): { toServer: Uint8Array; toPage: Uint8Array } {
  const both = hkdf(sha256, root, concat(sn, cn), CHANNEL_LABEL, KEY_BYTES * 2)
  return { toServer: both.slice(0, KEY_BYTES), toPage: both.slice(KEY_BYTES) }
}

// 16 zero bytes, then the number of frames so far as a big endian u64. It is not sent.
class Counter {
  private high = 0
  private low = 0

  nonce(): Uint8Array {
    const nonce = new Uint8Array(XNONCE_BYTES)
    const view = new DataView(nonce.buffer)
    view.setUint32(XNONCE_BYTES - COUNTER_BYTES, this.high)
    view.setUint32(XNONCE_BYTES - COUNTER_BYTES / 2, this.low)
    return nonce
  }

  next(): void {
    this.low = (this.low + 1) >>> 0
    if (this.low === 0) this.high++
  }
}

export class Sealer {
  private readonly counter = new Counter()
  private readonly key: Uint8Array

  constructor(key: Uint8Array) {
    this.key = key
  }

  // `ciphertext ‖ tag`
  seal(kind: Kind, plaintext: Uint8Array): Uint8Array {
    const sealed = xchacha20poly1305(this.key, this.counter.nonce(), Uint8Array.of(kind)).encrypt(plaintext)
    this.counter.next()
    return sealed
  }
}

export class Opener {
  private readonly counter = new Counter()
  private readonly key: Uint8Array

  constructor(key: Uint8Array) {
    this.key = key
  }

  // The next frame, in order. The kind is not sent: it is found by trying both.
  open(wire: Uint8Array): { kind: Kind; plaintext: Uint8Array } {
    if (wire.length < TAG_BYTES) throw new BadFrame('a frame is shorter than its tag')
    for (const kind of KINDS) {
      try {
        const plaintext = xchacha20poly1305(this.key, this.counter.nonce(), Uint8Array.of(kind)).decrypt(wire)
        this.counter.next()
        return { kind, plaintext }
      } catch {
        // the other kind, or not ours
      }
    }
    throw new BadFrame('a frame does not open')
  }
}
