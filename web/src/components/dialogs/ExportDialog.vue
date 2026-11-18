<script setup lang="ts">
// Write the transcript as markdown on the backend's machine, with directory/file Tab completion.
import { computed, onMounted, onUnmounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDialogSubmit } from '../../composables/useDialogSubmit.ts'
import { handleListKey, usePickSelection } from '../../composables/usePickSelection.ts'
import { Key } from '../../keys.ts'
import { RefError } from '../../net/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import ModalDialog from '../ModalDialog.vue'
import PickList from '../PickList.vue'
import TextField from '../TextField.vue'

const ROW_HEIGHT_PX = 32
const MAX_ROWS = 5
const LOOKUP_DELAY_MS = 120

const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const dialogs = useDialogsStore()
const notices = useNoticesStore()
const workspace = useWorkspaceStore()

const defaultFilename = `ref-session-${props.sessionId}.md`
const path = ref(defaultFilename)
const entries = ref<{ name: string; isDir: boolean }[]>([])
const field = useTemplateRef<InstanceType<typeof TextField>>('field')
const sessionCwd = computed(() => workspace.infoOf(props.sessionId)?.cwd ?? (workspace.cwd || undefined))

const split = computed(() => {
  const cut = Math.max(path.value.lastIndexOf('/'), path.value.lastIndexOf('\\'))
  return {
    folder: path.value.slice(0, cut + 1),
    begin: path.value.slice(cut + 1).toLowerCase(),
    separator: cut >= 0 ? path.value[cut]! : '/',
  }
})

const items = computed(() =>
  entries.value
    .filter((entry) => entry.name.toLowerCase().startsWith(split.value.begin))
    .map((entry) => ({
      key: entry.isDir ? `${entry.name}/` : entry.name,
      isDir: entry.isDir,
      name: entry.name,
    })),
)
const { selected, move } = usePickSelection(items)

let lookup: ReturnType<typeof setTimeout> | undefined
let lookedUp = 0

function isDirNotFound(err: unknown): boolean {
  if (err instanceof RefError && (err.code === 'not_found' || err.code === 'dir_not_found')) return true
  if (!(err instanceof Error)) return false
  return err.message.startsWith('Directory does not exist:') || err.message.startsWith('Not a directory:')
}

watch(
  [() => split.value.folder, sessionCwd],
  ([folder, cwd]) => {
    clearTimeout(lookup)
    if (folder === '' && !cwd) {
      entries.value = []
      return
    }
    lookup = setTimeout(async () => {
      const mine = ++lookedUp
      try {
        const result = await workspace.listDirs(folder || '.', { cwd, includeFiles: true })
        if (mine !== lookedUp) return
        entries.value = [
          ...result.dirs.map((name) => ({ name, isDir: true })),
          ...(result.files ?? []).map((name) => ({ name, isDir: false })),
        ]
      } catch (err) {
        if (mine !== lookedUp) return
        entries.value = []
        if (!isDirNotFound(err)) notices.pushError(err)
      }
    }, LOOKUP_DELAY_MS)
  },
  { immediate: true },
)

onMounted(() => field.value?.select())
onUnmounted(() => clearTimeout(lookup))

function accept(key: string | undefined): void {
  const chosen = items.value.find((item) => item.key === key)
  if (!chosen) return
  const suffix = chosen.isDir ? split.value.separator : ''
  path.value = `${split.value.folder}${chosen.name}${suffix}`
  field.value?.focus()
}

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return
  if (event.key === Key.Tab && !event.shiftKey && items.value.length > 0) {
    accept(selected.value)
    event.preventDefault()
    return
  }
  handleListKey(event, move)
}

const { busy, error, submit } = useDialogSubmit(
  async () => {
    const written = await workspace.exportSession(props.sessionId, path.value.trim())
    dialogs.close()
    notices.push('info', t('dialog.export.done', { path: written }))
  },
  () => path.value.trim() !== '',
)
watch(path, () => (error.value = ''))
</script>

<template>
  <ModalDialog :title="t('dialog.export.title')" @close="dialogs.close()">
    <p class="body">{{ t('dialog.export.body') }}</p>
    <TextField
      ref="field"
      v-model="path"
      :label="t('dialog.path.label')"
      :error="error"
      @enter="submit"
      @keydown="onKey"
    />
    <div v-if="items.length > 0" class="completions">
      <PickList
        :items="items"
        :selected="selected"
        :row-height="ROW_HEIGHT_PX"
        :max-rows="MAX_ROWS"
        :grouped="false"
        :empty="t('dialog.noMatch')"
        @hover="selected = $event"
        @pick="accept"
      >
        <template #row="{ item }">
          <span class="entry">
            <Icon :icon="item.isDir ? 'folder-16' : 'file-16'" class="icon" />{{ item.key }}
          </span>
        </template>
      </PickList>
    </div>
    <template #footer>
      <DialogButton :label="t('dialog.cancel')" @click="dialogs.close()" />
      <DialogButton
        kind="primary"
        :label="t('dialog.export.action')"
        :disabled="busy || path.trim() === ''"
        @click="submit"
      />
    </template>
  </ModalDialog>
</template>

<style scoped>
.body {
  margin: 0 0 12px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}
.completions {
  margin-top: 4px;
}

.entry {
  display: flex;
  align-items: center;
  gap: 8px;
}

.icon {
  flex: none;
  color: var(--text-muted);
}
</style>
