// Light or dark. Follows the system until the user toggles; from then on the choice is theirs
// (Theme.dark in Theme.qml). The choice lives in the settings store.

import { computed, watch } from 'vue'
import { useSettingsStore } from '../stores/settings.ts'

// Puts the theme on the page and keeps it there when the system or another tab changes it.
export function installTheme(): void {
  const settings = useSettingsStore()
  watch(
    () => settings.isDark,
    (dark) => (document.documentElement.dataset['theme'] = dark ? 'dark' : 'light'),
    { flush: 'sync', immediate: true },
  )
}

// Swaps the theme under a snapshot of the old look and fades the snapshot out (base.css), so
// every color crossfades at once without per-element transitions. Without view transitions
// the theme switches at once.
export function toggleTheme(): void {
  const settings = useSettingsStore()
  const change = () => settings.update((draft) => (draft.theme = settings.isDark ? 'light' : 'dark'))
  if (document.startViewTransition) document.startViewTransition(change)
  else change()
}

export function useTheme() {
  const settings = useSettingsStore()
  return { isDark: computed(() => settings.isDark) }
}
