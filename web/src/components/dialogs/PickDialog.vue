<script setup lang="ts" generic="T extends PickItem">
// A dialog that picks one of many rows by searching: models, agents, variants, sessions.
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { rankByQuery } from '../../commands/fuzzy.ts'
import { handleListKey, usePickSelection } from '../../composables/usePickSelection.ts'
import type { PickItem } from '../../composables/usePickSelection.ts'
import Icon from '../Icon.vue'
import ModalDialog from '../ModalDialog.vue'
import PickList from '../PickList.vue'

// Rows of the pick dialogs (models, agents, variants, sessions).
const PICK_ROW_HEIGHT_PX = 44
const PICK_MAX_ROWS = 8

const props = defineProps<{
  title: string
  searchLabel: string
  items: readonly T[]
  text: (item: T) => string // what the search looks at
  checked?: string // key of the current choice
  empty?: string // shown when items is empty
  noMatch?: string // shown when search filters out all items
}>()
const emit = defineEmits<{ pick: [item: T]; close: [] }>()

const { t } = useI18n()
const query = ref('')
const searching = computed(() => query.value.trim() !== '')
const shown = computed(() =>
  searching.value ? rankByQuery(props.items, query.value, props.text) : props.items,
)
const { selected, move } = usePickSelection(shown, () => props.checked)
const emptyLabel = computed(() => {
  if (props.items.length === 0) return props.empty ?? t('dialog.noMatch')
  return props.noMatch ?? t('dialog.noMatch')
})

const input = useTemplateRef<HTMLInputElement>('input')
onMounted(() => input.value?.focus())

function clearQuery(): void {
  query.value = ''
  input.value?.focus()
}

function pick(key: string | undefined): void {
  const item = shown.value.find((candidate) => candidate.key === key)
  if (item) emit('pick', item)
}

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return
  handleListKey(event, move, () => pick(selected.value))
}
</script>

<template>
  <ModalDialog :title="title" @close="emit('close')">
    <label class="search">
      <Icon icon="search-16" class="icon" />
      <input
        ref="input"
        v-model="query"
        type="text"
        class="field"
        spellcheck="false"
        autocomplete="off"
        :placeholder="searchLabel"
        :aria-label="searchLabel"
        @keydown="onKey"
      />
      <button
        v-if="query"
        type="button"
        class="clear"
        :aria-label="t('sidebar.clearSearch')"
        @click="clearQuery"
      >
        <Icon icon="close-16" />
      </button>
    </label>
    <slot name="toolbar" :selected="selected" />
    <PickList
      :items="shown"
      :selected="selected"
      :row-height="PICK_ROW_HEIGHT_PX"
      :max-rows="PICK_MAX_ROWS"
      :grouped="!searching"
      :empty="emptyLabel"
      @hover="selected = $event"
      @pick="pick"
    >
      <template v-if="items.length === 0 && $slots['empty-action']" #empty-action>
        <slot name="empty-action" />
      </template>
      <template #row="{ item }">
        <div class="row">
          <slot name="row" :item="item" />
          <Icon v-if="item.key === checked" icon="check-16" class="check" />
        </div>
      </template>
    </PickList>
  </ModalDialog>
</template>

<style scoped>
.search {
  display: flex;
  flex: none;
  align-items: center;
  gap: 10px;
  height: 52px;
  margin: 0 -22px 4px;
  padding: 0 22px;
  border-block: 1px solid var(--border);
}

.icon {
  color: var(--text-muted);
}

.field {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: 0;
  background: none;
  color: var(--text);
  font: inherit;
}

.clear {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.clear:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.row {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.check {
  flex: none;
  margin-left: auto;
  color: var(--accent-text);
}
</style>
