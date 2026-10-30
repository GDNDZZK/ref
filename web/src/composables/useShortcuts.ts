// The keyboard shortcuts of the app: the commands' own keys and Alt+arrows for the split view.

import { onBeforeUnmount, onMounted } from 'vue'
import { IS_MAC } from '../commands/platform.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import { isTyping } from './isTyping.ts'

const PALETTE = 'palette'
const ARROW_STEPS: Record<string, [number, number]> = {
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
}

export function useShortcuts(): void {
  const commands = useCommandsStore()
  const dialogs = useDialogsStore()
  const notices = useNoticesStore()
  const workspace = useWorkspaceStore()

  // Alt+arrows move the tile focus. On macOS they move the cursor by words in a text field.
  function moveFocus(event: KeyboardEvent): boolean {
    const step = ARROW_STEPS[event.code]
    const plainAlt = event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey
    if (!step || !plainAlt || workspace.mode !== 'split') return false
    if (IS_MAC && isTyping(event.target)) return false
    workspace.moveTileFocus(...step)
    return true
  }

  function onKeydown(event: KeyboardEvent): void {
    if (event.isComposing) return
    const command = commands.matchKey(event)
    // An open dialog owns the keyboard, except for the key that opens (and closes) the palette.
    if (dialogs.current && command?.id !== PALETTE) return
    if (command?.id === PALETTE && dialogs.current?.kind === PALETTE) {
      event.preventDefault()
      dialogs.close()
      return
    }
    if (command) {
      event.preventDefault()
      commands.run(command.id).catch(notices.pushError)
      return
    }
    if (!dialogs.current && moveFocus(event)) event.preventDefault()
  }

  onMounted(() => window.addEventListener('keydown', onKeydown))
  onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
}
