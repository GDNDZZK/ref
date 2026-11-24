// The dialog on screen. Opening one while another is open replaces it.

import { defineStore } from 'pinia'
import { shallowRef } from 'vue'

export type PeersTab = 'peers' | 'name' | 'inbox' | 'outbox'
export type ConfigSectionId = 'general' | 'providers' | 'agents' | 'permissions' | 'mcp' | 'commands'
export type SessionsDialogMode = 'switch' | 'close'

export type DialogRequest =
  | { kind: 'palette' }
  | { kind: 'models'; sessionId: string }
  | { kind: 'agents'; sessionId: string }
  | { kind: 'variants'; sessionId: string }
  | { kind: 'sessions'; mode?: SessionsDialogMode }
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
  | { kind: 'backend' }
  | { kind: 'stash' }
  | { kind: 'config'; section?: ConfigSectionId }

export const useDialogsStore = defineStore('dialogs', () => {
  const current = shallowRef<DialogRequest>()
  let onClosed: (() => void) | undefined
  let closeGuard: ((source: 'escape' | 'scrim' | 'button') => boolean) | undefined

  function open(request: DialogRequest): void {
    if (current.value && closeGuard && !closeGuard('button')) return
    closeGuard = undefined
    current.value = request
  }

  function close(): void {
    if (!current.value) return
    closeGuard = undefined
    current.value = undefined
    onClosed?.()
  }

  function requestClose(source: 'escape' | 'scrim' | 'button' = 'button'): void {
    if (closeGuard && !closeGuard(source)) return
    close()
  }

  function setCloseGuard(guard: ((source: 'escape' | 'scrim' | 'button') => boolean) | undefined): void {
    closeGuard = guard
  }

  // The dialog host sets what happens after a dialog closes: focus goes back to the composer.
  function setOnClosed(callback: () => void): void {
    onClosed = callback
  }

  return { current, open, close, requestClose, setCloseGuard, setOnClosed }
})
