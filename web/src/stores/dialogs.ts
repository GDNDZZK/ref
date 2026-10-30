// The dialog on screen. Opening one while another is open replaces it.

import { defineStore } from 'pinia'
import { shallowRef } from 'vue'

export type PeersTab = 'peers' | 'name' | 'inbox' | 'outbox'

export type DialogRequest =
  | { kind: 'palette' }
  | { kind: 'models'; sessionId: string }
  | { kind: 'agents'; sessionId: string }
  | { kind: 'variants'; sessionId: string }
  | { kind: 'sessions' }
  | { kind: 'newSession' }
  | { kind: 'rename'; sessionId: string }
  | { kind: 'cd'; sessionId: string }
  | { kind: 'export'; sessionId: string }
  | { kind: 'closeSession'; sessionId: string } // asks first: the session is still working
  | { kind: 'deleteHistory'; sessionId: string }
  | { kind: 'mcp' }
  | { kind: 'peers'; sessionId: string; tab: PeersTab; name?: string } // `name`: sets the peer name on open
  | { kind: 'help' }
  | { kind: 'settings' }

export const useDialogsStore = defineStore('dialogs', () => {
  const current = shallowRef<DialogRequest>()
  let onClosed: (() => void) | undefined

  function open(request: DialogRequest): void {
    current.value = request
  }

  function close(): void {
    if (!current.value) return
    current.value = undefined
    onClosed?.()
  }

  // The dialog host sets what happens after a dialog closes: focus goes back to the composer.
  function setOnClosed(callback: () => void): void {
    onClosed = callback
  }

  return { current, open, close, setOnClosed }
})
