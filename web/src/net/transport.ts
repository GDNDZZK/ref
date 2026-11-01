// The raw socket: text frames in, text frames out. Nothing here knows the protocol.

export interface TransportHandlers {
  opened(): void
  message(text: string): void
  closed(reason: string): void
}

export interface Transport {
  send(text: string): void
  close(): void
}

// Opens a connection; `handlers` are called until it closes.
export type TransportFactory = (handlers: TransportHandlers) => Transport

export function webSocketTransport(url: string): TransportFactory {
  return (handlers) => {
    const socket = new WebSocket(url)
    socket.onopen = () => handlers.opened()
    socket.onmessage = (event: MessageEvent) => {
      if (typeof event.data !== 'string') {
        handlers.closed('the server sent a binary frame')
        socket.close()
        return
      }
      handlers.message(event.data)
    }
    socket.onclose = (event) => handlers.closed(event.reason || `closed (${event.code})`)
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
