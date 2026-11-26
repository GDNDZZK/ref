<script setup lang="ts">
// Native <select> or editable combobox (`editable: true`) styled with theme tokens and Chevron.
// Preserves unknown configured values so viewing a form never silently overwrites custom ref.json strings.
import { computed, nextTick, onBeforeUnmount, ref, useTemplateRef, watch } from 'vue'
import { Key } from '../../keys.ts'
import Chevron from '../Chevron.vue'
import Icon from '../Icon.vue'

export interface SelectOption {
  value: string
  label: string
  disabled?: boolean
}

const POPUP_GAP = 4
const EDGE_MARGIN = 8
const MAX_POPUP_HEIGHT = 240
const MIN_POPUP_WIDTH = 160

const props = withDefaults(
  defineProps<{
    options: readonly SelectOption[]
    placeholder?: string
    disabled?: boolean
    label?: string
    id?: string
    monospace?: boolean
    editable?: boolean
  }>(),
  {
    placeholder: undefined,
    disabled: false,
    label: undefined,
    id: undefined,
    monospace: false,
    editable: false,
  },
)

const modelValue = defineModel<string>({ required: true })

const resolvedOptions = computed<SelectOption[]>(() => {
  const list: SelectOption[] = []
  const hasEmpty = props.options.some((option) => option.value === '')
  if (props.placeholder !== undefined && !hasEmpty) {
    list.push({ value: '', label: props.placeholder })
  }
  list.push(...props.options)

  const current = modelValue.value
  const hasCurrent = list.some((option) => option.value === current)
  if (current !== '' && !hasCurrent) {
    list.push({ value: current, label: current })
  }
  return list
})

const rootEl = useTemplateRef<HTMLElement>('rootEl')
const inputEl = useTemplateRef<HTMLInputElement>('inputEl')
const listboxEl = useTemplateRef<HTMLElement>('listboxEl')

const open = ref(false)
const isFiltering = ref(false)
const highlightedIndex = ref(-1)
const draft = ref(modelValue.value)
const popupStyle = ref<Record<string, string>>({})

let editingFromInput = false

const filteredOptions = computed<readonly SelectOption[]>(() => {
  if (!isFiltering.value) return props.options
  const needle = draft.value.trim().toLowerCase()
  if (needle === '') return props.options
  return props.options.filter((option) => `${option.label} ${option.value}`.toLowerCase().includes(needle))
})

function firstEnabledIndex(): number {
  return filteredOptions.value.findIndex((option) => !option.disabled)
}

function lastEnabledIndex(): number {
  const list = filteredOptions.value
  for (let i = list.length - 1; i >= 0; i -= 1) {
    const option = list[i]
    if (option && !option.disabled) return i
  }
  return -1
}

function matchingOrFirstIndex(value: string): number {
  const exact = filteredOptions.value.findIndex((option) => option.value === value && !option.disabled)
  if (exact >= 0) return exact
  return firstEnabledIndex()
}

function scrollHighlightedIntoView(): void {
  void nextTick(() => {
    const box = listboxEl.value
    if (!box || highlightedIndex.value < 0) return
    const items = box.querySelectorAll<HTMLElement>('[role="option"]')
    const target = items[highlightedIndex.value]
    target?.scrollIntoView?.({ block: 'nearest' })
  })
}

function updatePosition(): void {
  const anchor = rootEl.value
  if (!anchor) return
  const rect = anchor.getBoundingClientRect()
  const viewportWidth = window.innerWidth
  const viewportHeight = window.innerHeight

  const clampedWidth = Math.min(
    Math.max(rect.width, MIN_POPUP_WIDTH),
    Math.max(0, viewportWidth - 2 * EDGE_MARGIN),
  )
  const left = Math.max(EDGE_MARGIN, Math.min(rect.left, viewportWidth - clampedWidth - EDGE_MARGIN))

  const spaceBelow = viewportHeight - rect.bottom - POPUP_GAP - EDGE_MARGIN
  const spaceAbove = rect.top - POPUP_GAP - EDGE_MARGIN
  const contentHeight = listboxEl.value?.scrollHeight || MAX_POPUP_HEIGHT
  const desiredHeight = Math.min(MAX_POPUP_HEIGHT, contentHeight)

  const style: Record<string, string> = {
    left: `${left}px`,
    width: `${clampedWidth}px`,
  }

  if (spaceBelow < Math.min(desiredHeight, 160) && spaceAbove > spaceBelow) {
    const maxHeight = Math.max(120, Math.min(MAX_POPUP_HEIGHT, spaceAbove))
    style.bottom = `${viewportHeight - rect.top + POPUP_GAP}px`
    style.maxHeight = `${maxHeight}px`
  } else {
    const maxHeight = Math.max(120, Math.min(MAX_POPUP_HEIGHT, spaceBelow))
    style.top = `${rect.bottom + POPUP_GAP}px`
    style.maxHeight = `${maxHeight}px`
  }

  popupStyle.value = style
}

function openDropdown(showAll: boolean): void {
  if (props.disabled) return
  isFiltering.value = !showAll
  if (filteredOptions.value.length === 0) {
    open.value = false
    highlightedIndex.value = -1
    return
  }
  highlightedIndex.value = matchingOrFirstIndex(modelValue.value)
  open.value = true
  updatePosition()
  void nextTick(() => {
    updatePosition()
    scrollHighlightedIntoView()
  })
}

function closeDropdown(): void {
  open.value = false
  isFiltering.value = false
  highlightedIndex.value = -1
}

function toggleDropdown(): void {
  if (props.disabled || props.options.length === 0) return
  if (open.value) {
    closeDropdown()
    return
  }
  inputEl.value?.focus()
  openDropdown(true)
}

function pickOption(option: SelectOption): void {
  if (option.disabled) return
  editingFromInput = true
  draft.value = option.value
  modelValue.value = option.value
  closeDropdown()
  void nextTick(() => {
    editingFromInput = false
  })
}

function moveHighlight(delta: 1 | -1): void {
  const list = filteredOptions.value
  const len = list.length
  if (len === 0) return

  const index = highlightedIndex.value
  if (index < 0 || index >= len) {
    highlightedIndex.value = delta > 0 ? firstEnabledIndex() : lastEnabledIndex()
    scrollHighlightedIntoView()
    return
  }

  for (let step = 1; step <= len; step += 1) {
    const candidate = (index + delta * step + len) % len
    const option = list[candidate]
    if (option && !option.disabled) {
      highlightedIndex.value = candidate
      scrollHighlightedIntoView()
      return
    }
  }
}

function onInput(event: Event): void {
  const next = (event.target as HTMLInputElement).value
  editingFromInput = true
  draft.value = next
  isFiltering.value = true
  modelValue.value = next

  if (filteredOptions.value.length > 0) {
    open.value = true
    highlightedIndex.value = matchingOrFirstIndex(next)
    updatePosition()
    void nextTick(() => {
      updatePosition()
    })
  } else {
    open.value = false
    highlightedIndex.value = -1
  }

  void nextTick(() => {
    editingFromInput = false
  })
}

function onInputBlur(): void {
  editingFromInput = false
  draft.value = modelValue.value
  if (inputEl.value && inputEl.value.value !== modelValue.value) {
    inputEl.value.value = modelValue.value
  }
  closeDropdown()
}

function onInputKeydown(event: KeyboardEvent): void {
  if (props.disabled) return
  if (event.key === Key.ArrowDown) {
    event.preventDefault()
    if (!open.value) {
      openDropdown(true)
      return
    }
    moveHighlight(1)
    return
  }
  if (event.key === Key.ArrowUp) {
    event.preventDefault()
    if (!open.value) {
      openDropdown(true)
      return
    }
    moveHighlight(-1)
    return
  }
  if (event.key === Key.Enter) {
    if (event.isComposing || !open.value) return
    event.preventDefault()
    const picked = filteredOptions.value[highlightedIndex.value]
    if (picked && !picked.disabled) {
      pickOption(picked)
      return
    }
    closeDropdown()
    return
  }
  if (event.key === Key.Escape) {
    if (!open.value) return
    event.preventDefault()
    event.stopPropagation()
    closeDropdown()
  }
}

function onOptionHover(index: number, option: SelectOption): void {
  if (option.disabled) return
  highlightedIndex.value = index
}

function onPointerDown(event: PointerEvent): void {
  const target = event.target as Node | null
  if (!target) return
  if (rootEl.value?.contains(target) || listboxEl.value?.contains(target)) return
  closeDropdown()
}

function onWindowScroll(event: Event): void {
  const target = event.target as Node | null
  if (target && listboxEl.value && (target === listboxEl.value || listboxEl.value.contains(target))) {
    return
  }
  closeDropdown()
}

function onWindowResize(): void {
  closeDropdown()
}

function listen(): void {
  window.addEventListener('pointerdown', onPointerDown, true)
  window.addEventListener('scroll', onWindowScroll, true)
  window.addEventListener('resize', onWindowResize)
}

function unlisten(): void {
  window.removeEventListener('pointerdown', onPointerDown, true)
  window.removeEventListener('scroll', onWindowScroll, true)
  window.removeEventListener('resize', onWindowResize)
}

watch(open, (isOpen) => {
  if (isOpen) listen()
  else unlisten()
})

watch(
  () => filteredOptions.value.length,
  (len) => {
    if (!open.value) return
    if (len === 0) {
      open.value = false
      highlightedIndex.value = -1
      return
    }
    if (highlightedIndex.value >= len) {
      highlightedIndex.value = firstEnabledIndex()
    }
  },
)

watch(modelValue, (next) => {
  if (editingFromInput) return
  draft.value = next
  isFiltering.value = false
  if (open.value) closeDropdown()
})

watch(
  () => props.disabled,
  (isDisabled) => {
    if (isDisabled && open.value) closeDropdown()
  },
)

onBeforeUnmount(unlisten)
</script>

<template>
  <div v-if="!editable" class="select-field" :class="{ disabled }">
    <select
      :id="id"
      v-model="modelValue"
      class="select"
      :class="{ mono: monospace, muted: modelValue === '' && placeholder !== undefined }"
      :disabled="disabled"
      :aria-label="label"
    >
      <option
        v-for="option in resolvedOptions"
        :key="option.value"
        :value="option.value"
        :disabled="option.disabled"
      >
        {{ option.label }}
      </option>
    </select>
    <Chevron :size="12" class="chevron" />
  </div>

  <div v-else ref="rootEl" class="select-field editable" :class="{ disabled }">
    <input
      :id="id"
      ref="inputEl"
      type="text"
      role="combobox"
      class="combo-input"
      :class="{ mono: monospace, 'has-toggle': options.length > 0 }"
      spellcheck="false"
      autocomplete="off"
      :value="draft"
      :disabled="disabled"
      :placeholder="placeholder"
      :aria-label="label"
      :aria-expanded="open"
      aria-autocomplete="list"
      @input="onInput"
      @blur="onInputBlur"
      @keydown="onInputKeydown"
    />
    <button
      v-if="options.length > 0"
      type="button"
      class="toggle"
      tabindex="-1"
      aria-hidden="true"
      :disabled="disabled"
      @mousedown.prevent
      @click="toggleDropdown"
    >
      <Chevron :size="12" class="toggle-chevron" :class="{ open }" />
    </button>

    <Teleport to="body">
      <div
        v-if="open && filteredOptions.length > 0"
        ref="listboxEl"
        class="dropdown"
        role="listbox"
        :aria-label="label"
        :style="popupStyle"
        @mousedown.prevent
      >
        <button
          v-for="(option, index) in filteredOptions"
          :key="option.value"
          type="button"
          role="option"
          tabindex="-1"
          class="option"
          :class="{
            mono: monospace,
            active: index === highlightedIndex,
            selected: option.value === modelValue,
            disabled: option.disabled,
          }"
          :aria-selected="option.value === modelValue"
          :aria-disabled="option.disabled || undefined"
          @mousemove="onOptionHover(index, option)"
          @mousedown.prevent
          @click="pickOption(option)"
        >
          <span class="option-label">{{ option.label }}</span>
          <Icon v-if="option.value === modelValue" icon="check-16" class="option-check" />
        </button>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.select-field {
  position: relative;
  display: flex;
  align-items: center;
  width: 100%;
}

.select-field.disabled {
  opacity: var(--disabled-opacity);
}

.select {
  appearance: none;
  width: 100%;
  height: var(--field-height);
  padding: 0 32px 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  cursor: pointer;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.select.mono,
.combo-input.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.select.muted {
  color: var(--text-muted);
}

.select:focus,
.combo-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.select:disabled,
.combo-input:disabled {
  cursor: default;
}

.chevron {
  position: absolute;
  right: 11px;
  color: var(--text-muted);
  pointer-events: none;
  rotate: 90deg;
}

.combo-input {
  width: 100%;
  height: var(--field-height);
  padding: 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.combo-input.has-toggle {
  padding-right: 34px;
}

.toggle {
  position: absolute;
  right: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.toggle:not(:disabled):hover {
  background: var(--surface-hover);
  color: var(--text);
}

.toggle:disabled {
  cursor: default;
}

.toggle-chevron {
  rotate: 90deg;
  transition: rotate var(--motion-quick) var(--motion-ease);
}

.toggle-chevron.open {
  rotate: -90deg;
}

.dropdown {
  position: fixed;
  z-index: 40;
  overflow-y: auto;
  padding: 4px;
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow:
    inset 0 0 0 1px var(--border),
    0 6px 20px #0000002e;
}

.option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  min-height: 32px;
  padding: 5px 10px;
  border-radius: var(--radius-small);
  color: var(--text);
  font-size: var(--secondary-font-size);
  text-align: left;
  cursor: pointer;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.option.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.option.active {
  background: var(--surface-hover);
}

.option.selected {
  font-weight: 600;
}

.option.disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}

.option-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.option-check {
  flex: none;
  color: var(--accent-text);
}
</style>
