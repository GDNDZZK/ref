<script setup lang="ts">
// The to-do list of the session on screen, at the top of the sidebar (the TUI's Todo section).
// It grows with the list up to a third of the sidebar, then scrolls inside.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Todo } from '../proto/generated'
import { todoProgress } from '../shell/sessionRow.ts'
import Chevron from './Chevron.vue'
import TodoList from './TodoList.vue'

const props = withDefaults(
  defineProps<{
    todos: readonly Todo[] | undefined
    expanded?: boolean
    height?: number
    maxHeight?: string
  }>(),
  { expanded: true, height: 0, maxHeight: undefined },
)
const emit = defineEmits<{ toggle: [] }>()

const { t } = useI18n()
const progress = computed(() => todoProgress(props.todos))
const list = computed(() => [...(props.todos ?? [])])
const customHeight = computed(() => props.expanded && props.height > 0)
const slotStyle = computed(() => {
  if (!customHeight.value) return undefined
  return {
    '--todo-height': `${props.height}px`,
    ...(props.maxHeight ? { '--todo-max-height': props.maxHeight } : {}),
  }
})
</script>
<template>
  <Transition name="todo-fold">
    <div v-if="progress" class="todo-slot" :class="{ 'custom-height': customHeight }" :style="slotStyle">
      <div class="todo-fold">
        <section class="todo-section" :aria-label="t('sidebar.todo')">
          <h2 class="heading">
            <button type="button" class="section-toggle" :aria-expanded="expanded" @click="emit('toggle')">
              <span class="section-label">
                {{ t('sidebar.todo') }}
                <span class="count" :class="{ complete: progress.complete }"
                  >{{ progress.done }}/{{ progress.total }}</span
                >
              </span>
              <Chevron class="section-chevron" :class="{ open: expanded }" />
            </button>
          </h2>
          <div class="todo-body" :class="{ open: expanded }" :inert="!expanded || undefined">
            <div class="todo-body-fold">
              <TodoList class="list" :todos="list" />
            </div>
          </div>
        </section>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.todo-slot {
  display: grid;
  flex: 0 1 auto;
  grid-template-rows: 1fr;
  min-height: 0;
  max-height: calc(100% / var(--sidebar-todo-share));
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.todo-slot.custom-height {
  height: var(--todo-height);
  min-height: calc(var(--sidebar-section-height) + 38px);
  max-height: var(--todo-max-height, calc(100% - 2 * var(--sidebar-section-height)));
}

.todo-fold {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.todo-fold-enter-from,
.todo-fold-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
}

.todo-section {
  display: flex;
  flex: 0 1 auto;
  flex-direction: column;
  min-height: 0;
  margin-bottom: 4px;
}

.heading {
  flex: none;
  height: var(--sidebar-section-height);
  margin: 0;
  font-size: var(--small-font-size);
  font-weight: 400;
}

.section-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  height: 100%;
  padding: 0 10px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  text-align: left;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.section-toggle:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.section-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.count {
  margin-left: 6px;
}
.count.complete {
  color: var(--success-text);
}

.section-chevron {
  flex: none;
  color: var(--text-muted);
  transition: rotate var(--motion-standard) var(--motion-ease);
}

.section-chevron.open {
  rotate: 90deg;
}

.todo-body {
  display: grid;
  flex: 0 1 auto;
  grid-template-rows: 0fr;
  min-height: 0;
  opacity: 0;
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.todo-body.open {
  grid-template-rows: 1fr;
  opacity: 1;
}

.todo-body-fold {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.todo-section .list {
  min-height: 0;
  padding: 0 4px 2px 10px;
  overflow-y: auto;
  border-top: 0;
}
</style>
