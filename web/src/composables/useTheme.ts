// Light or dark. Follows the system until the user toggles; from then on the choice is theirs
// (Theme.dark in Theme.qml). The choice lives in the settings store.

import { computed, watch } from 'vue'
import type { ThemeChoice } from '../settings/model.ts'
import { crossfade } from './crossfade.ts'
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

export interface ThemeOrigin {
  x: number
  y: number
}

export function setTheme(choice: ThemeChoice, origin?: ThemeOrigin): void {
  const settings = useSettingsStore()
  const nextDark =
    choice === 'dark' || (choice === 'system' && window.matchMedia?.('(prefers-color-scheme: dark)').matches)
  const radial = origin ? { x: origin.x, y: origin.y, expand: !nextDark } : undefined
  void crossfade(() => settings.update((draft) => (draft.theme = choice)), 'slow', radial)
}

export function toggleTheme(origin?: ThemeOrigin): void {
  setTheme(useSettingsStore().isDark ? 'light' : 'dark', origin)
}

export function useTheme() {
  const settings = useSettingsStore()
  return { isDark: computed(() => settings.isDark) }
}
