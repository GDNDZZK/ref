<script setup lang="ts">
// The to-do list of the session on screen, at the top of the sidebar (the TUI's Todo section).
// It grows with the list up to a third of the sidebar, then scrolls inside.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Todo } from '../proto/generated'
import { todoProgress } from '../shell/sessionRow.ts'
import TodoList from './TodoList.vue'

const props = defineProps<{ todos: readonly Todo[] | undefined }>()

const { t } = useI18n()
const progress = computed(() => todoProgress(props.todos))
const list = computed(() => [...(props.todos ?? [])])
</script>

<template>
  <section v-if="progress" class="todo-section" :aria-label="t('sidebar.todo')">
    <h2 class="heading">
      {{ t('sidebar.todo') }}
      <span class="count" :class="{ complete: progress.complete }"
        >{{ progress.done }}/{{ progress.total }}</span
      >
    </h2>
    <TodoList class="list" :todos="list" />
  </section>
</template>

<style scoped>
.todo-section {
  display: flex;
  flex: 0 1 auto;
  flex-direction: column;
  min-height: 0;
  max-height: calc(100% / var(--sidebar-todo-share));
  margin-bottom: 4px;
}

.heading {
  display: flex;
  flex: none;
  align-items: center;
  gap: 8px;
  height: var(--sidebar-section-height);
  margin: 0;
  padding-left: 10px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 400;
}

.count.complete {
  color: var(--success-text);
}

.todo-section .list {
  min-height: 0;
  padding: 0 4px 2px 10px;
  overflow-y: auto;
  border-top: 0;
}
</style>
