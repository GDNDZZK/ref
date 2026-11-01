// The raw socket: text frames in, text frames out. Nothing here knows the protocol.

// What ref-web says about itself in the first sealed frame (see channel.ts).
export interface ServerInfo {
  version: string
  cwd: string
  mock: boolean
}

export interface TransportHandlers {
  opened(): void
  message(text: string): void
  closed(reason: string): void
}

export interface Transport {
  send(text: string): void
  close(): void
  // Set once the transport has opened, when the server tells it (only through ref-web).
  readonly info?: ServerInfo
}

// Opens a connection; `handlers` are called until it closes.
export type TransportFactory = (handlers: TransportHandlers) => Transport

// The WebSocket as it is: text or binary messages, and the close code.
export interface SocketHandlers {
  opened(): void
  message(data: string | Uint8Array): void
  closed(code: number, reason: string): void
}

export interface Socket {
  send(data: string | Uint8Array): void
  close(): void
}

export type SocketFactory = (handlers: SocketHandlers) => Socket

export function webSocket(url: string): SocketFactory {
  return (handlers) => {
    const socket = new WebSocket(url)
    socket.binaryType = 'arraybuffer'
    socket.onopen = () => handlers.opened()
    socket.onmessage = (event: MessageEvent) => {
      const data: unknown = event.data
      handlers.message(typeof data === 'string' ? data : new Uint8Array(data as ArrayBuffer))
    }
    socket.onclose = (event) => handlers.closed(event.code, event.reason)
    return { send: (data) => socket.send(data as string | BufferSource), close: () => socket.close() }
  }
}

// Text frames only, without ref-web's channel: for a dev build that talks to a ref-server.
export function webSocketTransport(url: string): TransportFactory {
  const open = webSocket(url)
  return (handlers) => {
    const socket = open({
      opened: () => handlers.opened(),
      message: (data) => {
        if (typeof data !== 'string') {
          handlers.closed('the server sent a binary frame')
          socket.close()
          return
        }
        handlers.message(data)
      },
      closed: (code, reason) => handlers.closed(reason || `closed (${code})`),
    })
    return { send: (text) => socket.send(text), close: () => socket.close() }
  }
}

// The ref-server a dev build talks to directly (`VITE_REF_URL`), if there is one.
export function directServerUrl(): string | undefined {
  return import.meta.env.VITE_REF_URL as string | undefined
}

// ws(s)://<this host>/ws, which ref-web serves, unless a dev build points at a ref-server.
export function defaultServerUrl(): string {
  const configured = directServerUrl()
  if (configured) return configured
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
  return `${scheme}://${location.host}/ws`
}
