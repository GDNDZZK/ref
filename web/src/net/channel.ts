// The channel to ref-web: the key handshake, then every message sealed. A TransportFactory, so
// connection.ts and everything above it keep seeing plain text frames.
//
//   ref-web: {"t":"hello","v":1,"salt":…,"sn":…}
//   page:    {"t":"auth","cn":…,"mac":…}        mac proves the key without sending it
//   ref-web: sealed frame 0 {"version","cwd","mock"}; the transport is now open
//   then:    one sealed binary message per protocol frame, both ways

import {
  BadFrame,
  KIND_BINARY,
  KIND_TEXT,
  NONCE_BYTES,
  Opener,
  PROTOCOL_VERSION,
  SALT_BYTES,
  Sealer,
  authMac,
  channelKeys,
  fromHex,
  toHex,
  utf8,
} from './channelCrypto.ts'
import type { ServerInfo, SocketFactory, Transport, TransportFactory } from './transport.ts'

// Close codes of ref-web (crates/web/src/tunnel.rs).
export const CLOSE_KEY_REJECTED = 4401

// Where the roots come from: the key store.
export interface KeySource {
  // The root made with ref-web's salt (hex); undefined when the key has to be entered.
  rootFor(saltHex: string): Promise<Uint8Array | undefined>
  // ref-web did not accept the key.
  rejected(): void
}

export type RandomBytes = (length: number) => Uint8Array

function cryptoRandom(length: number): Uint8Array {
  return crypto.getRandomValues(new Uint8Array(length))
}

interface Hello {
  t: string
  v: number
  salt: string
  sn: string
}

type Phase = 'hello' | 'frame0' | 'open' | 'over'

export function channelTransport(
  open: SocketFactory,
  keys: KeySource,
  random: RandomBytes = cryptoRandom,
): TransportFactory {
  return (handlers) => {
    let phase: Phase = 'hello'
    let sealer: Sealer | undefined
    let opener: Opener | undefined
    let info: ServerInfo | undefined
    // Messages are handled one after the other: the first one waits for the key derivation.
    let queue: Promise<void> = Promise.resolve()

    // Reports the end once, and stops the socket.
    const end = (reason: string): void => {
      if (phase === 'over') return
      phase = 'over'
      socket.close()
      handlers.closed(reason)
    }

    const answerHello = async (text: string): Promise<void> => {
      const hello = parseHello(text)
      const salt = hello && fromHex(hello.salt, SALT_BYTES)
      const sn = hello && fromHex(hello.sn, NONCE_BYTES)
      if (!hello || hello.v !== PROTOCOL_VERSION || !salt || !sn) {
        end('this ref-web speaks another channel version')
        return
      }
      const root = await keys.rootFor(hello.salt)
      if (!root) {
        end('a key is needed')
        return
      }
      const cn = random(NONCE_BYTES)
      const mac = authMac(root, sn, cn)
      const { toServer, toPage } = channelKeys(root, sn, cn)
      sealer = new Sealer(toServer)
      opener = new Opener(toPage)
      phase = 'frame0'
      socket.send(JSON.stringify({ t: 'auth', cn: toHex(cn), mac: toHex(mac) }))
    }

    const receiveSealed = (wire: Uint8Array): void => {
      let opened
      try {
        opened = opener!.open(wire)
      } catch (error) {
        if (!(error instanceof BadFrame)) throw error
        end('a frame from ref-web did not open')
        return
      }
      const text = new TextDecoder().decode(opened.plaintext)
      if (phase === 'frame0') {
        const parsed = parseInfo(text)
        if (!parsed) {
          end('ref-web sent no start information')
          return
        }
        info = parsed
        phase = 'open'
        handlers.opened()
        return
      }
      if (opened.kind === KIND_BINARY) {
        end('the server sent a binary frame')
        return
      }
      handlers.message(text)
    }

    const receive = async (data: string | Uint8Array): Promise<void> => {
      if (phase === 'over') return
      if (phase === 'hello') {
        if (typeof data !== 'string') return end('ref-web did not start with a hello')
        return answerHello(data)
      }
      if (typeof data === 'string') return end('ref-web sent an unsealed message')
      receiveSealed(data)
    }

    const socket = open({
      opened: () => {}, // the transport opens when frame 0 has come
      message: (data) => {
        queue = queue.then(() => receive(data)).catch((error) => end(String(error)))
      },
      closed: (code, reason) => {
        if (code === CLOSE_KEY_REJECTED) keys.rejected()
        if (phase === 'over') return
        phase = 'over'
        handlers.closed(reason || `closed (${code})`)
      },
    })

    const transport: Transport = {
      send(text) {
        if (phase !== 'open' || !sealer) throw new Error('the channel is not open')
        socket.send(sealer.seal(KIND_TEXT, utf8(text)))
      },
      close: () => {
        phase = 'over'
        socket.close()
      },
      get info() {
        return info
      },
    }
    return transport
  }
}

function parseHello(text: string): Hello | undefined {
  try {
    const hello = JSON.parse(text) as Partial<Hello>
    const wellFormed =
      hello.t === 'hello' &&
      typeof hello.v === 'number' &&
      typeof hello.salt === 'string' &&
      typeof hello.sn === 'string'
    return wellFormed ? (hello as Hello) : undefined
  } catch {
    return undefined
  }
}

function parseInfo(text: string): ServerInfo | undefined {
  try {
    const info = JSON.parse(text) as Partial<ServerInfo>
    const wellFormed =
      typeof info.version === 'string' && typeof info.cwd === 'string' && typeof info.mock === 'boolean'
    return wellFormed ? (info as ServerInfo) : undefined
  } catch {
    return undefined
  }
}
