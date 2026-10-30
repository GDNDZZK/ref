<script setup lang="ts">
// The command palette (CommandPalette.qml): search the commands, run one. A command that takes an
// argument turns the search row into an argument prompt.
import { computed, nextTick, onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { rankByQuery } from '../commands/fuzzy.ts'
import { labelText } from '../commands/label.ts'
import type { Command, CommandGroup } from '../commands/types.ts'
import { usePickSelection } from '../composables/usePickSelection.ts'
import type { PickItem } from '../composables/usePickSelection.ts'
import type { IconKey } from '../icons/generated/icons.ts'
import { metrics } from '../theme/generated/theme.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useNoticesStore } from '../stores/notices.ts'
import Icon from './Icon.vue'
import PickList from './PickList.vue'
import ShortcutChips from './ShortcutChips.vue'

const ROW_HEIGHT_PX = metrics.paletteRowHeight
const MAX_ROWS = metrics.paletteMaxRows
const GROUP_ORDER: readonly CommandGroup[] = ['session', 'view', 'help', 'custom']
const DEFAULT_ICON: IconKey = 'slash-16'
// Command id -> icon; the commands without one use the slash.
const ICONS: Record<string, IconKey> = {
  new: 'chat-plus-16',
  'new-in': 'folder-search-16',
  sessions: 'chat-list-16',
  models: 'model-16',
  agents: 'agent-16',
  variant: 'variant-16',
  compact: 'recall-16',
  undo: 'undo-16',
  rename: 'pencil-16',
  cd: 'folder-16',
  close: 'chat-x-16',
  'close-session': 'chat-x-16',
  export: 'file-text-16',
  copy: 'copy-16',
  peers: 'at-sign-16',
  mcp: 'mcp-16',
  help: 'info-16',
}

const { t } = useI18n()
const commands = useCommandsStore()
const dialogs = useDialogsStore()
const notices = useNoticesStore()

interface Entry extends PickItem {
  command: Command
  title: string
}

const query = ref('')
const chosen = ref<Command>() // the command whose argument is being typed
const input = useTemplateRef<HTMLInputElement>('input')

const all = computed<Entry[]>(() =>
  commands
    .listed()
    .map(({ command, enabled }) => ({
      key: command.id,
      group: t(`command.group.${command.group}`),
      disabled: !enabled,
      command,
      title: labelText(command.title),
    }))
    .sort((a, b) => GROUP_ORDER.indexOf(a.command.group) - GROUP_ORDER.indexOf(b.command.group)),
)

const searching = computed(() => query.value.trim() !== '' && !chosen.value)
const shown = computed<Entry[]>(() => {
  if (chosen.value) return []
  if (!searching.value) return all.value
  return rankByQuery(all.value, query.value, (entry) => `${entry.title} ${entry.command.id}`)
})
const { selected, move } = usePickSelection(shown)

onMounted(() => input.value?.focus())

// Runs the command. Commands that open a dialog replace the palette; for the others it closes here.
function run(command: Command, args = ''): void {
  const opened = dialogs.current
  const done = commands.run(command.id, args)
  if (dialogs.current === opened) dialogs.close()
  done.catch(notices.pushError)
}

function pick(key: string | undefined): void {
  const entry = shown.value.find((candidate) => candidate.key === key)
  if (!entry) return
  if (!entry.command.args) return run(entry.command)
  chosen.value = entry.command
  query.value = ''
  void nextTick(() => input.value?.focus())
}

function backToList(): void {
  chosen.value = undefined
  query.value = ''
}

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return
  if (chosen.value) {
    const backspaceBack = event.key === 'Backspace' && query.value === ''
    if (event.key === 'Enter') run(chosen.value, query.value.trim())
    else if (event.key === 'Escape' || backspaceBack) backToList()
    else return
    event.preventDefault()
    event.stopPropagation() // Esc would otherwise close the dialog as well
    return
  }
  if (event.key === 'ArrowDown') move(1)
  else if (event.key === 'ArrowUp') move(-1)
  else if (event.key === 'Enter') pick(selected.value)
  else return
  event.preventDefault()
}
</script>

<template>
  <section class="palette" role="dialog" aria-modal="true" :aria-label="t('palette.placeholder')">
    <div class="search">
      <span v-if="chosen" class="chip">{{ labelText(chosen.title) }}</span>
      <Icon v-else icon="search-16" class="icon" />
      <input
        ref="input"
        v-model="query"
        type="text"
        class="field"
        spellcheck="false"
        autocomplete="off"
        :placeholder="chosen?.args?.usage ?? t('palette.placeholder')"
        :aria-label="t('palette.placeholder')"
        @keydown="onKey"
      />
    </div>
    <div class="body">
      <p v-if="chosen" class="hint">
        <span class="usage">/{{ chosen.id }} {{ chosen.args?.usage }}</span>
      </p>
      <PickList
        v-else
        :items="shown"
        :selected="selected"
        :row-height="ROW_HEIGHT_PX"
        :max-rows="MAX_ROWS"
        :grouped="!searching"
        :empty="t('palette.noMatch')"
        @hover="selected = $event"
        @pick="pick"
      >
        <template #row="{ item }">
          <Icon :icon="ICONS[item.key] ?? DEFAULT_ICON" class="icon" />
          <span class="title">{{ item.title }}</span>
          <span v-if="item.command.slash" class="slash">/{{ item.command.id }}</span>
          <ShortcutChips v-if="item.command.shortcut" :shortcut="item.command.shortcut" class="keys" />
        </template>
      </PickList>
    </div>
  </section>
</template>

<style scoped>
.palette {
  width: min(var(--palette-width), calc(100vw - 24px));
  overflow: hidden;
  border-radius: var(--radius-large);
  background: var(--surface);
  box-shadow:
    inset 0 0 0 1px var(--border),
    0 12px 40px #00000040;
}

.search {
  display: flex;
  align-items: center;
  gap: 10px;
  height: var(--palette-search-height);
  padding: 0 18px;
  border-bottom: 1px solid var(--border);
}

.icon {
  flex: none;
  color: var(--text-muted);
}

.chip {
  flex: none;
  padding: 3px 10px;
  border-radius: 12px;
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
  font-size: var(--small-font-size);
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

.body {
  padding: 8px;
}

.hint {
  margin: 0;
  padding: 12px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.slash {
  margin-left: 8px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.keys {
  margin-left: auto;
}
</style>
