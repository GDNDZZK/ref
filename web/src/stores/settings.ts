// All persistent settings, in localStorage and shared between tabs (the `storage` event).

import { defineStore } from 'pinia'
import { computed, ref, toRaw } from 'vue'
import { pickLocale, translate } from '../i18n/index.ts'
import type { LocaleName } from '../i18n/index.ts'
import { STORAGE_KEY, notifyKinds as kindsFor, parseSettings } from '../settings/model.ts'
import type { Settings } from '../settings/model.ts'
import { useNoticesStore } from './notices.ts'

export const useSettingsStore = defineStore('settings', () => {
  const loaded = parseSettings(localStorage.getItem(STORAGE_KEY))
  const settings = ref<Settings>(loaded.settings)
  if (loaded.corrupt) useNoticesStore().push('warn', translate('storage.settingsCorrupt'))

  // Another tab saved: take its values. `newValue` is null when the key was removed.
  window.addEventListener('storage', (event) => {
    if (event.key === STORAGE_KEY || event.key === null)
      settings.value = parseSettings(event.newValue).settings
  })

  // Changes a copy, then stores and publishes it.
  function update(change: (draft: Settings) => void): void {
    const draft = structuredClone(toRaw(settings.value))
    change(draft)
    settings.value = draft
    localStorage.setItem(STORAGE_KEY, JSON.stringify(draft))
  }

  const systemQuery = window.matchMedia('(prefers-color-scheme: dark)')
  const systemDark = ref(systemQuery.matches)
  systemQuery.addEventListener('change', (event) => (systemDark.value = event.matches))

  const isDark = computed(() =>
    settings.value.theme === 'system' ? systemDark.value : settings.value.theme === 'dark',
  )

  const locale = computed<LocaleName>(() =>
    settings.value.language === 'system' ? pickLocale(navigator.languages) : settings.value.language,
  )

  // For Hello.notify and SetNotify: empty when notifications are off.
  const notifyKinds = computed(() => kindsFor(settings.value.notifications))

  return { settings, isDark, locale, notifyKinds, update }
})
