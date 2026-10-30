<script setup lang="ts">
// Keyboard shortcuts and commands, generated from the command registry so they cannot drift from
// the real ones. The keys the composer handles itself are listed under "Writing".
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { labelText } from '../../commands/label.ts'
import { useCommandsStore } from '../../stores/commands.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import ModalDialog from '../ModalDialog.vue'
import ShortcutChips from '../ShortcutChips.vue'

const { t } = useI18n()
const commands = useCommandsStore()
const dialogs = useDialogsStore()

const shortcuts = computed(() =>
  commands.commands.flatMap((command) =>
    command.shortcut ? [{ id: command.id, title: labelText(command.title), shortcut: command.shortcut }] : [],
  ),
)

const slashes = computed(() => commands.slashEntries())

const writing = computed(() => [
  { id: 'send', title: t('dialog.help.send'), keys: ['Enter'] },
  { id: 'newLine', title: t('dialog.help.newLine'), keys: ['Shift', 'Enter'] },
  { id: 'interrupt', title: t('dialog.help.interrupt'), keys: ['Esc', 'Esc'] },
  { id: 'cycleAgent', title: t('dialog.help.cycleAgent'), keys: ['Tab'] },
  { id: 'history', title: t('dialog.help.history'), keys: ['↑', '↓'] },
  { id: 'pasteImage', title: t('dialog.help.pasteImage'), keys: ['Ctrl', 'V'] },
  { id: 'moveFocus', title: t('dialog.help.moveFocus'), keys: ['Alt', '←', '→', '↑', '↓'] },
])
</script>

<template>
  <ModalDialog :title="t('dialog.help.title')" width="wide" @close="dialogs.close()">
    <h3 class="section">{{ t('dialog.help.keyboard') }}</h3>
    <div v-for="row in shortcuts" :key="row.id" class="row">
      <span>{{ row.title }}</span>
      <ShortcutChips :shortcut="row.shortcut" />
    </div>

    <h3 class="section">{{ t('dialog.help.writing') }}</h3>
    <div v-for="row in writing" :key="row.id" class="row">
      <span>{{ row.title }}</span>
      <ShortcutChips :keys="row.keys" />
    </div>

    <h3 class="section">{{ t('dialog.help.commands') }}</h3>
    <div v-for="entry in slashes" :key="entry.name" class="row">
      <span class="command">
        /{{ entry.name }}
        <span v-if="entry.usage" class="muted">{{ entry.usage }}</span>
        <span v-for="alias in entry.aliases" :key="alias" class="muted">/{{ alias }}</span>
      </span>
      <span class="muted title">{{ labelText(entry.title) }}</span>
    </div>
  </ModalDialog>
</template>

<style scoped>
.section {
  margin: 14px 0 4px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 400;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-height: 30px;
  font-size: calc(var(--small-font-size) + 1px);
}

.command {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  font-family: var(--font-mono);
}

.muted {
  color: var(--text-muted);
}

.title {
  text-align: right;
}
</style>
