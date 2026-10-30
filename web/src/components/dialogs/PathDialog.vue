<script setup lang="ts">
// A directory path with completion from the backend's file system: /cd and "new session in…".
import { computed, onMounted, onUnmounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { usePickSelection } from '../../composables/usePickSelection.ts'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { describeError } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import ModalDialog from '../ModalDialog.vue'
import PickList from '../PickList.vue'
import TextField from '../TextField.vue'

const ROW_HEIGHT_PX = 32
const MAX_ROWS = 6
const LOOKUP_DELAY_MS = 120

const props = defineProps<{ mode: 'cd' | 'newSession'; sessionId?: string }>()
const { t } = useI18n()
const connection = useConnectionStore()
const dialogs = useDialogsStore()
const workspace = useWorkspaceStore()

const startPath = (props.sessionId ? workspace.infoOf(props.sessionId)?.cwd : undefined) ?? workspace.cwd
const path = ref(startPath)
const error = ref('')
const busy = ref(false)
const dirs = ref<string[]>([])
const field = useTemplateRef<InstanceType<typeof TextField>>('field')

// "/home/me/pro" is the folder "/home/me/" and the beginning "pro" of a name in it.
const split = computed(() => {
  const cut = Math.max(path.value.lastIndexOf('/'), path.value.lastIndexOf('\\'))
  return {
    folder: path.value.slice(0, cut + 1),
    begin: path.value.slice(cut + 1).toLowerCase(),
    separator: cut >= 0 ? path.value[cut]! : '/',
  }
})

const items = computed(() =>
  dirs.value
    .filter((name) => name.toLowerCase().startsWith(split.value.begin))
    .map((name) => ({ key: name })),
)
const { selected, move } = usePickSelection(items)

let lookup: ReturnType<typeof setTimeout> | undefined
let lookedUp = 0

// Asks for the folder's sub-directories shortly after the typing stops.
watch(
  () => split.value.folder,
  (folder) => {
    clearTimeout(lookup)
    if (folder === '') return void (dirs.value = [])
    lookup = setTimeout(async () => {
      const mine = ++lookedUp
      try {
        const result = await connection.getApi().listDirs(folder)
        if (mine === lookedUp) dirs.value = result.dirs
      } catch {
        if (mine === lookedUp) dirs.value = [] // a folder that does not exist (yet) has no completions
      }
    }, LOOKUP_DELAY_MS)
  },
  { immediate: true },
)

onMounted(() => field.value?.select())
onUnmounted(() => clearTimeout(lookup))

function accept(name: string | undefined): void {
  if (name === undefined) return
  path.value = `${split.value.folder}${name}${split.value.separator}`
  field.value?.focus()
}

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return
  if (event.key === 'Tab' && !event.shiftKey && items.value.length > 0) accept(selected.value)
  else if (event.key === 'ArrowDown') move(1)
  else if (event.key === 'ArrowUp') move(-1)
  else return
  event.preventDefault()
}

async function submit(): Promise<void> {
  const target = path.value.trim()
  if (target === '' || busy.value) return
  busy.value = true
  try {
    if (props.mode === 'cd' && props.sessionId) await workspace.setSessionCwd(props.sessionId, target)
    else await workspace.newSession({ cwd: target })
    dialogs.close()
  } catch (failure) {
    error.value = describeError(failure)
  } finally {
    busy.value = false
  }
}

watch(path, () => (error.value = ''))
</script>

<template>
  <ModalDialog :title="t(`dialog.${mode}.title`)" @close="dialogs.close()">
    <TextField
      ref="field"
      v-model="path"
      :label="t('dialog.path.label')"
      :error="error"
      @enter="submit"
      @keydown="onKey"
    />
    <div class="completions">
      <PickList
        :items="items"
        :selected="selected"
        :row-height="ROW_HEIGHT_PX"
        :max-rows="MAX_ROWS"
        :grouped="false"
        :empty="t('dialog.path.noDirs')"
        @hover="selected = $event"
        @pick="accept"
      >
        <template #row="{ item }">
          <span class="entry"><Icon icon="folder-16" class="icon" />{{ item.key }}</span>
        </template>
      </PickList>
    </div>
    <template #footer>
      <DialogButton :label="t('dialog.cancel')" @click="dialogs.close()" />
      <DialogButton
        kind="primary"
        :label="t(`dialog.${mode}.action`)"
        :disabled="busy || path.trim() === ''"
        @click="submit"
      />
    </template>
  </ModalDialog>
</template>

<style scoped>
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
