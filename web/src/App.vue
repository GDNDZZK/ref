<script setup lang="ts">
// The window (Main.qml): the open session, with the theme toggle and the notices on top.
import ConnectionBanner from './components/ConnectionBanner.vue'
import NoticeList from './components/NoticeList.vue'
import SessionPane from './components/SessionPane.vue'
import ThemeToggle from './components/ThemeToggle.vue'
import { usePermissionKeys } from './composables/usePermissionKeys.ts'
import { useWorkspaceStore } from './stores/workspace.ts'

const workspace = useWorkspaceStore()
usePermissionKeys()
</script>

<template>
  <div class="app">
    <ConnectionBanner />
    <SessionPane v-if="workspace.current" :key="workspace.current.id" :session="workspace.current" />
    <ThemeToggle class="theme-toggle" />
    <NoticeList />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.theme-toggle {
  position: fixed;
  top: 14px;
  right: 14px;
}
</style>
