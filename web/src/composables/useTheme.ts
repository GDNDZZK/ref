// Light or dark. Follows the system until the user toggles; from then on the choice is theirs
// (Theme.dark in Theme.qml). Stored in localStorage and shared between tabs.

import { computed, ref, watch } from 'vue'

export type ThemeChoice = 'system' | 'light' | 'dark'

const STORAGE_KEY = 'ref.theme'
const CHOICES: readonly string[] = ['system', 'light', 'dark']

function readChoice(): ThemeChoice {
  const stored = localStorage.getItem(STORAGE_KEY)
  return stored !== null && CHOICES.includes(stored) ? (stored as ThemeChoice) : 'system'
}

const systemQuery = window.matchMedia('(prefers-color-scheme: dark)')
const choice = ref<ThemeChoice>(readChoice())
const systemDark = ref(systemQuery.matches)

systemQuery.addEventListener('change', (event) => (systemDark.value = event.matches))
window.addEventListener('storage', (event) => {
  if (event.key === STORAGE_KEY) choice.value = readChoice()
})

const isDark = computed(() => (choice.value === 'system' ? systemDark.value : choice.value === 'dark'))

export function applyTheme(): void {
  document.documentElement.dataset['theme'] = isDark.value ? 'dark' : 'light'
}

// Follows the system and other tabs.
watch(isDark, applyTheme, { flush: 'sync' })

// Swaps the theme under a snapshot of the old look and fades the snapshot out (base.css), so
// every color crossfades at once without per-element transitions. Without view transitions
// the theme switches at once.
export function toggleTheme(): void {
  const change = () => {
    choice.value = isDark.value ? 'light' : 'dark'
    localStorage.setItem(STORAGE_KEY, choice.value)
    applyTheme()
  }
  if (document.startViewTransition) document.startViewTransition(change)
  else change()
}

export function useTheme() {
  return { isDark }
}
