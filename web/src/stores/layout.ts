// How the window is laid out: narrow windows show the sidebar as a drawer over the content and
// have no split view.

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { useSettingsStore } from './settings.ts'

export const NARROW_BELOW_PX = 800

export const useLayoutStore = defineStore('layout', () => {
  const settings = useSettingsStore()

  const query = window.matchMedia(`(max-width: ${NARROW_BELOW_PX - 0.02}px)`)
  const narrow = ref(query.matches)
  query.addEventListener('change', (event) => (narrow.value = event.matches))

  const drawerOpen = ref(false) // the drawer starts closed and is not remembered
  // Whether the sidebar is on screen: the saved setting in a wide window, the drawer in a narrow one.
  const sidebarVisible = computed(() => (narrow.value ? drawerOpen.value : settings.settings.sidebarOpen))

  function toggleSidebar(): void {
    if (narrow.value) drawerOpen.value = !drawerOpen.value
    else settings.update((draft) => (draft.sidebarOpen = !draft.sidebarOpen))
  }

  // After choosing something in the drawer, so that the content is visible again.
  function closeDrawer(): void {
    drawerOpen.value = false
  }

  return { narrow, sidebarVisible, toggleSidebar, closeDrawer }
})
