<script setup lang="ts">
// The todo list, always shown under the tool header.
import type { Todo } from '../proto/generated'
import TodoMarker from './TodoMarker.vue'

defineProps<{ todos: Todo[] }>()
</script>

<template>
  <ul v-if="todos.length > 0" class="todos">
    <li
      v-for="(todo, index) in todos"
      :key="index"
      class="todo"
      :class="{ finished: todo.status === 'completed' || todo.status === 'cancelled' }"
    >
      <TodoMarker :status="todo.status" />
      <span class="text">{{ todo.content }}</span>
    </li>
  </ul>
</template>

<style scoped>
.todos {
  margin: 0;
  padding: 0 14px 6px;
  list-style: none;
  border-top: 1px solid var(--border);
}

.todo {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 26px;
  font-size: 13px;
}

.finished .text {
  color: var(--text-muted);
  text-decoration: line-through;
}
</style>
