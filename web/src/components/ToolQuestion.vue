<script setup lang="ts">
// The questions of a `question` tool call, inside its card: the options to pick from, an inline
// field for an own answer, and for several questions or a multiple choice a tab per question
// and a confirm step. The keys act on the same form (usePermissionKeys); this is the mouse side.
import { computed, nextTick, shallowRef, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { FormOutcome, QuestionForm } from '../model/questionForm.ts'
import Icon from './Icon.vue'

const props = defineProps<{ form: QuestionForm | undefined; sending: boolean }>()
const emit = defineEmits<{ answer: [answers: string[][] | null] }>()

const { t } = useI18n()
const field = useTemplateRef<HTMLInputElement[]>('field')

// Kept while the section folds away after the answer.
const shown = shallowRef(props.form)
watch(
  () => props.form,
  (form) => form && (shown.value = form),
)

const current = computed(() => shown.value)
const question = computed(() => current.value?.questions[current.value.tab])
const rows = computed(() => {
  const form = current.value
  const item = question.value
  if (!form || !item) return []
  const options = item.options.map((option, row) => ({
    row,
    label: option.label,
    description: option.description,
    isDefault: option.default,
    custom: false,
  }))
  const own = {
    row: options.length,
    label: t('question.custom'),
    description: '',
    isDefault: false,
    custom: true,
  }
  return item.custom ? [...options, own] : options
})
const confirmLines = computed(() => {
  const form = current.value
  if (!form) return []
  const answers = form.answers()
  return form.questions.map((item, index) => ({ header: item.header, answer: answers[index]!.join(', ') }))
})

const hintKey = computed(() => {
  if (current.value?.onConfirmTab) return 'question.hintConfirm'
  return current.value?.tabbed ? 'question.hintTabbed' : 'question.hintSingle'
})

function finish(outcome: FormOutcome): void {
  if (outcome === 'submit') emit('answer', current.value!.answers())
  else if (outcome === 'dismiss') emit('answer', null)
}

function pick(row: number): void {
  const form = current.value
  if (form) finish(form.pick(form.tab, row, false))
}

function typed(event: Event): void {
  current.value?.setCustomText(current.value.tab, (event.target as HTMLInputElement).value)
}

function onFieldEnter(event: KeyboardEvent): void {
  if (event.isComposing || !current.value) return
  finish(current.value.commitCustom())
}

// The cursor goes into the field when the custom row is chosen, and leaves it again with Esc.
watch(
  () => current.value?.editing,
  async (editing) => {
    await nextTick()
    const input = field.value?.[0]
    if (editing) input?.focus()
    else if (input && document.activeElement === input) input.blur()
  },
)
</script>

<template>
  <div class="question" :class="{ open: form !== undefined, sending }">
    <div class="fold">
      <div v-if="current" class="section">
        <div v-if="current.tabbed" class="tabs" role="tablist">
          <button
            v-for="(item, index) in current.questions"
            :key="index"
            type="button"
            role="tab"
            class="tab"
            :class="{ active: current.tab === index }"
            :aria-selected="current.tab === index"
            @click="current.goToTab(index)"
          >
            {{ item.header }}
          </button>
          <button
            type="button"
            role="tab"
            class="tab"
            :class="{ active: current.onConfirmTab }"
            :aria-selected="current.onConfirmTab"
            @click="current.goToTab(current.questions.length)"
          >
            {{ t('question.confirm') }}
          </button>
        </div>

        <template v-if="!current.onConfirmTab && question">
          <div class="title">? {{ question.header }}</div>
          <p class="text">{{ question.question }}</p>
          <ul class="options" :role="question.multiple ? 'group' : 'radiogroup'">
            <li
              v-for="item in rows"
              :key="item.row"
              class="option"
              :class="{ cursor: current.choices[current.tab]!.cursor === item.row }"
              :role="question.multiple ? 'checkbox' : 'radio'"
              :aria-checked="current.isChosen(current.tab, item.row)"
              @mouseenter="current.highlight(current.tab, item.row)"
              @click="pick(item.row)"
            >
              <span class="number">{{ item.row + 1 }}</span>
              <span
                class="mark"
                :class="{ box: question.multiple, on: current.isChosen(current.tab, item.row) }"
              >
                <Icon v-if="question.multiple && current.isChosen(current.tab, item.row)" icon="check-16" />
              </span>
              <span class="label">{{ item.label }}</span>
              <span v-if="item.isDefault" class="default">{{ t('question.default') }}</span>
              <span v-if="item.description" class="description">{{ item.description }}</span>
              <input
                v-if="item.custom && current.choices[current.tab]!.customPicked"
                ref="field"
                class="field"
                data-question-field
                type="text"
                :value="current.choices[current.tab]!.customText"
                :placeholder="t('question.customPlaceholder')"
                :aria-label="t('question.custom')"
                @click.stop
                @focus="current.editing = true"
                @blur="current.editing = false"
                @input="typed"
                @keydown.enter.prevent="onFieldEnter"
                @keydown.esc.prevent.stop="current.leaveCustom()"
              />
            </li>
          </ul>
        </template>

        <ul v-else class="summary">
          <li v-for="(line, index) in confirmLines" :key="index" :class="{ unanswered: line.answer === '' }">
            <span class="summary-header">{{ line.header }}</span>
            <span>{{ line.answer === '' ? t('question.unanswered') : line.answer }}</span>
          </li>
        </ul>

        <div class="footer">
          <span class="hint">{{ t(hintKey) }}</span>
          <button type="button" class="action" @click="emit('answer', null)">
            {{ t('question.dismiss') }}
          </button>
          <button
            v-if="current.onConfirmTab"
            type="button"
            class="action primary"
            @click="emit('answer', current.answers())"
          >
            {{ t('question.submit') }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.question {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--motion-standard) var(--motion-ease);
}

.question.open {
  grid-template-rows: 1fr;
}

.question.sending {
  opacity: 0.6;
  pointer-events: none;
}

.fold {
  min-height: 0;
  overflow: hidden;
}

.section {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 14px;
  border-top: 1px solid var(--border);
}

.tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.tab {
  height: 26px;
  padding: 0 10px;
  border-radius: 13px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.tab:hover {
  background: var(--surface-hover);
}

.tab.active {
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
  color: var(--text);
}

.title {
  color: var(--warning);
  font-weight: 600;
}

.text {
  margin: 0;
  overflow-wrap: anywhere;
}

.options,
.summary {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.option {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 8px;
  padding: 6px 8px;
  border-radius: var(--radius-small);
  cursor: pointer;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.option.cursor {
  background: var(--surface-hover);
}

.number {
  width: 14px;
  color: var(--text-muted);
  font: 11px var(--font-mono);
  text-align: right;
}

.mark {
  display: grid;
  place-items: center;
  flex: none;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  box-shadow: inset 0 0 0 1.5px var(--text-muted);
  color: var(--on-accent);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    box-shadow var(--motion-quick) var(--motion-ease);
}

.mark.box {
  border-radius: 4px;
}

.mark.on {
  background: var(--accent);
  box-shadow: inset 0 0 0 1.5px var(--accent);
}

.mark:not(.box).on {
  background: radial-gradient(circle, var(--on-accent) 0 28%, var(--accent) 32%);
}

.label {
  font-weight: 600;
}

.default {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.description {
  flex: 1 1 12em;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.field {
  flex: 1 1 100%;
  min-width: 0;
  height: 28px;
  margin-left: 38px;
  padding: 0 8px;
  border: 0;
  border-radius: var(--radius-small);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  outline: 0;
}

.field::placeholder {
  color: var(--text-muted);
}

.field:focus {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.summary li {
  display: flex;
  gap: 12px;
  padding: 4px 8px;
}

.summary-header {
  flex: none;
  min-width: 6em;
  color: var(--text-muted);
}

.unanswered {
  color: var(--warning);
}

.footer {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

.hint {
  margin-right: auto;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.action {
  height: 28px;
  padding: 0 12px;
  border-radius: var(--radius-small);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.action:hover {
  background: var(--surface-hover);
}

.action.primary {
  background: var(--accent);
  box-shadow: none;
  color: var(--on-accent);
}

.action.primary:hover {
  background: color-mix(in srgb, var(--accent) 85%, var(--text));
}
</style>
