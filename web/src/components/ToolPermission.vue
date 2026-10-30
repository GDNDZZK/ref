<script setup lang="ts">
// The permission request inside a tool card: what will run and three answers (ToolBlock.qml).
// Keys act on the oldest request of the session (usePermissionKeys); the buttons are for the mouse.
import { computed, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PermissionRequest } from '../proto/generated'
import { stringArg } from '../model/toolContent.ts'
import CommandBox from './CommandBox.vue'
import Icon from './Icon.vue'

const props = defineProps<{ request: PermissionRequest | undefined; waiting: number }>()
const emit = defineEmits<{ answer: [allow: boolean, always: boolean] }>()

const { t } = useI18n()

// Kept while the section folds away after the answer.
const shown = shallowRef(props.request)
watch(
  () => props.request,
  (request) => request && (shown.value = request),
)

const command = computed(() => stringArg(shown.value?.input, 'command'))
const path = computed(() => stringArg(shown.value?.input, 'path'))

const CHOICES = [
  { tone: 'success', icon: 'check-16', label: 'tool.allow', key: 'a', allow: true, always: false },
  { tone: 'accent', icon: 'check-all-16', label: 'tool.allowSession', key: 's', allow: true, always: true },
  { tone: 'danger', icon: 'close-16', label: 'tool.deny', key: 'd', allow: false, always: false },
] as const
</script>

<template>
  <div class="permission" :class="{ open: request !== undefined }">
    <div class="fold">
      <div class="section">
        <CommandBox v-if="command !== undefined" :command="command" />
        <div v-if="path !== undefined" class="path">{{ path }}</div>
        <div class="choices">
          <button
            v-for="choice in CHOICES"
            :key="choice.key"
            type="button"
            class="choice"
            :class="choice.tone"
            @click="emit('answer', choice.allow, choice.always)"
          >
            <Icon :icon="choice.icon" />
            <span class="label">{{ t(choice.label) }}</span>
            <kbd>{{ choice.key }}</kbd>
          </button>
          <span v-if="waiting > 0" class="more">{{ t('tool.moreWaiting', { count: waiting }) }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.permission {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--motion-standard) var(--motion-ease);
}

.permission.open {
  grid-template-rows: 1fr;
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

.path {
  color: var(--info);
  font: var(--code-font-size) / 1.45 var(--font-mono);
  overflow-wrap: anywhere;
}

.choices {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.choice {
  --tone: var(--accent);
  display: flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 10px;
  border-radius: var(--radius-small);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--tone) 50%, transparent);
  color: var(--tone);
  font-size: var(--small-font-size);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.choice.success {
  --tone: var(--success);
}

.choice.danger {
  --tone: var(--danger);
}

.choice:hover {
  background: color-mix(in srgb, var(--tone) 12%, transparent);
}

.choice:active {
  background: color-mix(in srgb, var(--tone) 22%, transparent);
}

kbd {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  box-shadow: inset 0 0 0 1px var(--tone);
  font: 11px var(--font-mono);
}

.more {
  margin-left: auto;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}
</style>
