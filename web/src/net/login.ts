// Whether the browser's login to ref-web is gone, which a WebSocket handshake cannot tell us: the
// browser hides the status of a refused upgrade. An ordinary request to ref-web shows it.

import { directServerUrl } from './transport.ts'

export const UNAUTHORIZED = 401

export async function loginRequired(): Promise<boolean> {
  if (directServerUrl()) return false // a dev build talks to a ref-server, with no login
  try {
    const response = await fetch('/api/info', { cache: 'no-store' })
    return response.status === UNAUTHORIZED
  } catch {
    return false // ref-web is not reachable: keep trying
  }
}
