<script setup lang="ts" generic="T extends PickItem">
// A dialog that picks one of many rows by searching: models, agents, variants, sessions.
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { rankByQuery } from '../../commands/fuzzy.ts'
import { usePickSelection } from '../../composables/usePickSelection.ts'
import type { PickItem } from '../../composables/usePickSelection.ts'
import Icon from '../Icon.vue'
import ModalDialog from '../ModalDialog.vue'
import PickList from '../PickList.vue'

const ROW_HEIGHT_PX = 44
const MAX_ROWS = 8

const props = defineProps<{
  title: string
  searchLabel: string
  items: readonly T[]
  text: (item: T) => string // what the search looks at
  checked?: string // key of the current choice
  empty?: string
}>()
const emit = defineEmits<{ pick: [item: T]; close: [] }>()

const { t } = useI18n()
const query = ref('')
const searching = computed(() => query.value.trim() !== '')
const shown = computed(() =>
  searching.value ? rankByQuery(props.items, query.value, props.text) : props.items,
)
const { selected, move } = usePickSelection(shown, props.checked)

const input = useTemplateRef<HTMLInputElement>('input')
onMounted(() => input.value?.focus())

function pick(key: string | undefined): void {
  const item = shown.value.find((candidate) => candidate.key === key)
  if (item) emit('pick', item)
}

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return
  if (event.key === 'ArrowDown') move(1)
  else if (event.key === 'ArrowUp') move(-1)
  else if (event.key === 'Enter') pick(selected.value)
  else return
  event.preventDefault()
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
    </label>
    <PickList
      :items="shown"
      :selected="selected"
      :row-height="ROW_HEIGHT_PX"
      :max-rows="MAX_ROWS"
      :grouped="!searching"
      :empty="empty ?? t('palette.noMatch')"
      @hover="selected = $event"
      @pick="pick"
    >
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
  color: var(--accent);
}
</style>
