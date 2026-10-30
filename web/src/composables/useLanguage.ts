// Keeps the interface language (and the page's `lang`) on the setting, including changes from other tabs.

import { watch } from 'vue'
import type { createAppI18n } from '../i18n/index.ts'
import { useSettingsStore } from '../stores/settings.ts'

export function installLanguage(i18n: ReturnType<typeof createAppI18n>): void {
  const settings = useSettingsStore()
  watch(
    () => settings.locale,
    (locale) => {
      i18n.global.locale.value = locale
      document.documentElement.lang = locale
    },
    { immediate: true },
  )
}
