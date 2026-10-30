<script setup lang="ts">
// The paths `find` returned: an icon, the directory dimmed and the name.
import type { FoundPath } from '../model/toolContent.ts'
import Icon from './Icon.vue'

defineProps<{ paths: FoundPath[] }>()
</script>

<template>
  <div class="paths">
    <div v-for="path in paths" :key="path.directory + path.name" class="path">
      <Icon :icon="path.isDir ? 'folder-16' : 'file-16'" class="icon" />
      <span class="directory">{{ path.directory }}</span>
      <span class="name">{{ path.name }}</span>
    </div>
  </div>
</template>

<style scoped>
.path {
  display: flex;
  align-items: center;
  gap: 0;
  height: 24px;
  font: var(--small-font-size) var(--font-mono);
  white-space: pre;
}

.icon {
  margin-right: 8px;
  color: var(--text-muted);
}

.directory {
  color: var(--text-muted);
}
</style>
