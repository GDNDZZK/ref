// How the window is laid out: narrow windows show the sidebar as a drawer over the content and
// have no split view.

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { clamp } from '../clamp.ts'
import type { HistoryCreatorFilter } from '../settings/model.ts'
import { metrics } from '../theme/generated/theme.ts'
import { useSettingsStore } from './settings.ts'

const NARROW_BELOW_PX = 800
const FRACTIONAL_PX_EPSILON = 0.02 // so the query does not match at exactly NARROW_BELOW_PX
const MAX_SIDEBAR_RATIO = 0.45

export const useLayoutStore = defineStore('layout', () => {
  const settings = useSettingsStore()

  const query = window.matchMedia(`(max-width: ${NARROW_BELOW_PX - FRACTIONAL_PX_EPSILON}px)`)
  const narrow = ref(query.matches)
  query.addEventListener('change', (event) => (narrow.value = event.matches))

  const windowWidth = ref(window.innerWidth)
  window.addEventListener('resize', () => (windowWidth.value = window.innerWidth))

  const drawerOpen = ref(false) // the drawer starts closed and is not remembered
  // Whether the sidebar is on screen: the saved setting in a wide window, the drawer in a narrow one.
  const sidebarVisible = computed(() => (narrow.value ? drawerOpen.value : settings.settings.sidebarOpen))
  const maxSidebarWidth = computed(() =>
    Math.max(
      metrics.sidebarMinWidth,
      Math.floor(Math.min(metrics.sidebarMaxWidth, windowWidth.value * MAX_SIDEBAR_RATIO)),
    ),
  )
  const sidebarWidth = computed(() => clampWidth(settings.settings.sidebarWidth))
  const sidebarTodoExpanded = computed(() => settings.settings.sidebarTodoExpanded)
  const sidebarRunningExpanded = computed(() => settings.settings.sidebarRunningExpanded)
  const sidebarHistoryExpanded = computed(() => settings.settings.sidebarHistoryExpanded)
  const sidebarTodoHeight = computed(() => settings.settings.sidebarTodoHeight)
  const sidebarRunningHeight = computed(() => settings.settings.sidebarRunningHeight)
  const sidebarHistoryCreatorFilter = computed(() => settings.settings.sidebarHistoryCreatorFilter)
  const sidebarHistoryCurrentDir = computed(() => settings.settings.sidebarHistoryCurrentDir)
  function clampWidth(value: number): number {
    return clamp(Math.round(value), metrics.sidebarMinWidth, maxSidebarWidth.value)
  }

  function setSidebarWidth(value: number): void {
    if (!Number.isFinite(value)) return
    const next = clampWidth(value)
    if (next === settings.settings.sidebarWidth) return
    settings.update((draft) => (draft.sidebarWidth = next))
  }

  function resetSidebarWidth(): void {
    if (settings.settings.sidebarWidth === metrics.sidebarWidth) return
    settings.update((draft) => (draft.sidebarWidth = metrics.sidebarWidth))
  }

  function setSidebarTodoHeight(px: number): void {
    if (!Number.isFinite(px)) return
    const next = Math.max(0, Math.round(px)) || 0
    if (next === settings.settings.sidebarTodoHeight) return
    settings.update((draft) => (draft.sidebarTodoHeight = next))
  }

  function resetSidebarTodoHeight(): void {
    if (settings.settings.sidebarTodoHeight === 0) return
    settings.update((draft) => (draft.sidebarTodoHeight = 0))
  }

  function setSidebarRunningHeight(px: number): void {
    if (!Number.isFinite(px)) return
    const next = Math.max(0, Math.round(px)) || 0
    if (next === settings.settings.sidebarRunningHeight) return
    settings.update((draft) => (draft.sidebarRunningHeight = next))
  }

  function resetSidebarRunningHeight(): void {
    if (settings.settings.sidebarRunningHeight === 0) return
    settings.update((draft) => (draft.sidebarRunningHeight = 0))
  }
  function toggleSidebar(): void {
    if (narrow.value) drawerOpen.value = !drawerOpen.value
    else settings.update((draft) => (draft.sidebarOpen = !draft.sidebarOpen))
  }

  function toggleSidebarTodo(): void {
    settings.update((draft) => (draft.sidebarTodoExpanded = !draft.sidebarTodoExpanded))
  }

  function toggleSidebarRunning(): void {
    settings.update((draft) => (draft.sidebarRunningExpanded = !draft.sidebarRunningExpanded))
  }

  function toggleSidebarHistory(): void {
    settings.update((draft) => (draft.sidebarHistoryExpanded = !draft.sidebarHistoryExpanded))
  }

  function setSidebarHistoryCreatorFilter(value: HistoryCreatorFilter): void {
    if (value === settings.settings.sidebarHistoryCreatorFilter) return
    settings.update((draft) => (draft.sidebarHistoryCreatorFilter = value))
  }

  function setSidebarHistoryCurrentDir(value: boolean): void {
    if (value === settings.settings.sidebarHistoryCurrentDir) return
    settings.update((draft) => (draft.sidebarHistoryCurrentDir = value))
  }

  function toggleSidebarHistoryCurrentDir(): void {
    setSidebarHistoryCurrentDir(!settings.settings.sidebarHistoryCurrentDir)
  }

  // After choosing something in the drawer, so that the content is visible again.
  function closeDrawer(): void {
    drawerOpen.value = false
  }

  return {
    narrow,
    sidebarVisible,
    sidebarWidth,
    sidebarTodoExpanded,
    sidebarRunningExpanded,
    sidebarHistoryExpanded,
    sidebarTodoHeight,
    sidebarRunningHeight,
    sidebarHistoryCreatorFilter,
    sidebarHistoryCurrentDir,
    setSidebarWidth,
    resetSidebarWidth,
    setSidebarTodoHeight,
    resetSidebarTodoHeight,
    setSidebarRunningHeight,
    resetSidebarRunningHeight,
    toggleSidebar,
    toggleSidebarTodo,
    toggleSidebarRunning,
    toggleSidebarHistory,
    setSidebarHistoryCreatorFilter,
    setSidebarHistoryCurrentDir,
    toggleSidebarHistoryCurrentDir,
    closeDrawer,
  }
})
