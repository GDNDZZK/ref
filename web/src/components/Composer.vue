<script setup lang="ts">
// The input box of one session (Composer.qml): queue strip above, then the box with attachments,
// text, the tag row and the send button. The logic is in SessionComposer; this file is keys,
// paste and drop, and layout.
//
//   Enter send · Shift+Enter, Ctrl+J new line · Tab / Shift+Tab agent · Esc Esc interrupt
//   ↑ ↓ history · `/` commands · `@` files · `!` shell · paste or drop images
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionComposer } from '../composer/sessionComposer.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import { components } from '../theme/generated/components.ts'
import AttachmentStrip from './AttachmentStrip.vue'
import CompletionPopup from './CompletionPopup.vue'
import ComposerTags from './ComposerTags.vue'
import QueueStrip from './QueueStrip.vue'
import SendButton from './SendButton.vue'

const props = defineProps<{ session: ChatSession; composer: SessionComposer }>()
const emit = defineEmits<{ submitted: [] }>()

const {
  padding: PADDING,
  maxInputHeight: MAX_INPUT_HEIGHT,
  tagRowHeight: TAG_ROW_HEIGHT,
  textGap: TEXT_TAG_GAP,
  inputLeftInset: TEXT_LEFT_EXTRA,
  thumbRowGap: THUMB_ROW_GAP,
} = components.composer
const { thumbSize: THUMB_SIZE } = components.attachmentStrip
const SEND_BUTTON_SIZE = 32

const { t } = useI18n()
const connection = useConnectionStore()
const commands = useCommandsStore()
const workspace = useWorkspaceStore()
const notices = useNoticesStore()

const draft = computed(() => props.composer.draft)
const input = useTemplateRef<HTMLTextAreaElement>('input')
const root = useTemplateRef<HTMLElement>('root')
const inputHeight = ref(SEND_BUTTON_SIZE)
const dragDepth = ref(0) // drag events of the children come in pairs; the box is hovered while > 0

const canSend = computed(() => draft.value.text.trim() !== '' || draft.value.attachments.length > 0)
const stripHeight = computed(() => (draft.value.attachments.length > 0 ? THUMB_SIZE + THUMB_ROW_GAP : 0))
const boxHeight = computed(
  () => 2 * PADDING + stripHeight.value + inputHeight.value + TEXT_TAG_GAP + TAG_ROW_HEIGHT,
)
const interruptHint = computed(() => props.composer.interruptArmed.value)
const placeholder = computed(() => {
  if (connection.state !== 'open') return t('composer.placeholderWaiting')
  if (draft.value.shell) return t('composer.placeholderShell')
  return props.session.busy ? t('composer.placeholderQueue') : t('composer.placeholder')
})

function measure(): void {
  const area = input.value
  if (!area) return
  // Vue owns the inline height; put it back so a measure without a change leaves it alone.
  const applied = area.style.height
  area.style.height = '0'
  const wanted = Math.max(SEND_BUTTON_SIZE, area.scrollHeight)
  area.style.height = applied
  inputHeight.value = Math.min(wanted, MAX_INPUT_HEIGHT)
}

// The composer moves the caret too (history, completion, restored drafts).
async function showCaret(): Promise<void> {
  await nextTick()
  measure()
  const area = input.value
  const caret = props.composer.caret
  if (area && document.activeElement === area && area.selectionStart !== caret) {
    area.setSelectionRange(caret, caret)
  }
}

watch(() => draft.value.text, showCaret)

function sync(): void {
  const area = input.value
  if (area) props.composer.input(area.value, area.selectionStart)
}

function onInput(): void {
  sync()
  requestAnimationFrame(measure)
}

function submit(): void {
  if (!canSend.value) return
  emit('submitted')
  void props.composer.submit()
  requestAnimationFrame(measure)
}

function onSendButton(): void {
  if (props.session.busy) void props.composer.abort()
  else submit()
}

// --- keys ---------------------------------------------------------------------------------

function isPlain(event: KeyboardEvent): boolean {
  return !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey
}

// Keys while the list is open. Tab inserts the choice, Enter also runs a command.
function onListKey(event: KeyboardEvent): boolean {
  const ctrlOnly = event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey
  const up = (isPlain(event) && event.key === 'ArrowUp') || (ctrlOnly && event.key === 'p')
  const down = (isPlain(event) && event.key === 'ArrowDown') || (ctrlOnly && event.key === 'n')
  if (up || down) {
    props.composer.moveCompletion(up ? -1 : 1)
    return true
  }

  const isAccept = (event.key === 'Tab' || event.key === 'Enter') && !event.altKey && !event.ctrlKey
  if (!isAccept || event.metaKey) return false
  const caret = props.composer.acceptCompletion(event.key === 'Enter')
  if (caret !== undefined) void showCaret()
  return true
}

// ↑ walks back through the history from an empty box, from the very start of the text, and while
// browsing from the first line; ↓ walks forward from the last line while browsing.
function onHistoryKey(event: KeyboardEvent, area: HTMLTextAreaElement): boolean {
  const isArrow = event.key === 'ArrowUp' || event.key === 'ArrowDown'
  if (!isArrow || !isPlain(event) || area.selectionStart !== area.selectionEnd) return false

  const caret = area.selectionStart
  const browsing = props.composer.browsingHistory
  if (event.key === 'ArrowUp') {
    const onFirstLine = !area.value.slice(0, caret).includes('\n')
    const atStart = caret === 0
    if (area.value !== '' && !atStart && !(browsing && onFirstLine)) return false
    props.composer.historyPrev()
  } else {
    const onLastLine = !area.value.slice(caret).includes('\n')
    if (!browsing || !onLastLine) return false
    props.composer.historyNext()
  }
  void showCaret()
  return true
}

function onEscape(event: KeyboardEvent): boolean {
  if (event.key !== 'Escape') return false
  props.composer.escape()
  return true
}

function onEnter(event: KeyboardEvent): boolean {
  if (event.key !== 'Enter' || event.shiftKey || event.altKey) return false
  submit()
  return true
}

// Ctrl+J: a line break, like Shift+Enter.
function onLineBreak(event: KeyboardEvent, area: HTMLTextAreaElement): boolean {
  if (!event.ctrlKey || event.key !== 'j') return false
  area.setRangeText('\n', area.selectionStart, area.selectionEnd, 'end')
  onInput()
  return true
}

function onTab(event: KeyboardEvent): boolean {
  const hasModifier = event.ctrlKey || event.metaKey || event.altKey
  if (event.key !== 'Tab' || hasModifier) return false
  workspace.cycleAgent(props.session.id, event.shiftKey ? -1 : 1).catch(notices.pushError)
  return true
}

// `!` into an empty box starts shell mode; Backspace on the empty box leaves it.
function onShellKey(event: KeyboardEvent): boolean {
  const empty = draft.value.text === ''
  const hasModifier = event.ctrlKey || event.metaKey || event.altKey
  if (event.key === '!' && !hasModifier && empty && !draft.value.shell) {
    props.composer.enterShell()
    return true
  }
  if (event.key === 'Backspace' && empty && draft.value.shell) {
    props.composer.leaveShell()
    return true
  }
  return false
}

// Ctrl+C without a selection empties the box (with a selection it copies, as everywhere).
function onClear(event: KeyboardEvent, area: HTMLTextAreaElement): boolean {
  const noSelection = area.selectionStart === area.selectionEnd
  const hasContent = draft.value.text !== '' || draft.value.shell
  if (!event.ctrlKey || event.key !== 'c' || !noSelection || !hasContent) return false
  props.composer.clear()
  return true
}

function onKeydown(event: KeyboardEvent): void {
  const area = input.value
  const keyCodeComposing = 229
  if (!area || event.isComposing || event.keyCode === keyCodeComposing) return
  if (props.composer.completion && onListKey(event)) return event.preventDefault()

  const handlers: ((event: KeyboardEvent, area: HTMLTextAreaElement) => boolean)[] = [
    onEscape,
    onEnter,
    onLineBreak,
    onTab,
    onHistoryKey,
    onShellKey,
    onClear,
  ]
  const handled = handlers.some((handler) => handler(event, area))
  if (handled) event.preventDefault()
}

// The caret moved without typing (arrow keys, a click): the list is for the word under it.
function onCaretMoved(): void {
  sync()
}

// --- images -------------------------------------------------------------------------------

async function attach(files: readonly File[]): Promise<void> {
  sync()
  await props.composer.attachFiles(files)
  input.value?.focus()
  void showCaret()
}

function imagesOf(files: FileList | undefined): File[] {
  return [...(files ?? [])].filter((file) => file.type.startsWith('image/'))
}

// An image on the clipboard becomes an attachment; text that comes with it wins, as when
// copying cells from a spreadsheet.
function onPaste(event: ClipboardEvent): void {
  const data = event.clipboardData
  const images = imagesOf(data?.files)
  if (images.length === 0 || data?.getData('text/plain').trim()) return
  event.preventDefault()
  void attach(images)
}

function hasFiles(event: DragEvent): boolean {
  return event.dataTransfer?.types.includes('Files') ?? false
}

function onDragEnter(event: DragEvent): void {
  if (!hasFiles(event)) return
  event.preventDefault()
  dragDepth.value++
}

function onDragOver(event: DragEvent): void {
  if (hasFiles(event)) event.preventDefault()
}

function onDragLeave(event: DragEvent): void {
  if (hasFiles(event)) dragDepth.value = Math.max(0, dragDepth.value - 1)
}

function onDrop(event: DragEvent): void {
  if (!hasFiles(event)) return
  event.preventDefault()
  dragDepth.value = 0
  const files = [...(event.dataTransfer?.files ?? [])]
  if (files.length > 0) void attach(files)
}

// --- tags ---------------------------------------------------------------------------------

function openDialog(commandId: 'agents' | 'models'): void {
  commands.run(commandId, '', props.session.id).catch((error) => notices.pushError(error))
}

// The prompt comes back into the box: the caret goes there, ready to edit.
async function takeBack(messageId: string): Promise<void> {
  await props.composer.removeQueued(messageId)
  input.value?.focus()
  void showCaret()
}

function pickCompletion(index: number): void {
  props.composer.selectCompletion(index)
  props.composer.acceptCompletion(true)
  input.value?.focus()
  void showCaret()
}

// Wrapping depends on the width, so a resize can change the height wanted.
const resizeObserver = new ResizeObserver(measure)
onMounted(() => {
  if (root.value) resizeObserver.observe(root.value)
  measure()
})
onBeforeUnmount(() => resizeObserver.disconnect())

defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div class="composer">
    <QueueStrip :messages="session.queued" :removing="composer.removingQueued" @remove="takeBack" />
    <div class="box-wrap">
      <CompletionPopup
        :completion="composer.completion"
        @hover="(index) => composer.selectCompletion(index)"
        @pick="pickCompletion"
      />
      <div
        ref="root"
        class="box"
        :class="{ shell: draft.shell, dragging: dragDepth > 0 }"
        :style="{ height: `${boxHeight}px`, padding: `${PADDING}px` }"
        @dragenter="onDragEnter"
        @dragover="onDragOver"
        @dragleave="onDragLeave"
        @drop="onDrop"
      >
        <div class="strip-slot" :style="{ height: `${stripHeight}px` }">
          <AttachmentStrip
            :attachments="draft.attachments"
            @remove="(token) => composer.removeAttachment(token)"
          />
        </div>
        <textarea
          ref="input"
          class="input"
          rows="1"
          :value="draft.text"
          :placeholder="placeholder"
          :aria-label="t('composer.label')"
          :style="{ height: `${inputHeight}px`, marginLeft: `${TEXT_LEFT_EXTRA}px` }"
          @input="onInput"
          @keydown="onKeydown"
          @keyup="onCaretMoved"
          @click="onCaretMoved"
          @paste="onPaste"
        />
        <div class="tag-row" :style="{ height: `${TAG_ROW_HEIGHT}px` }">
          <ComposerTags
            :session="session"
            :shell="draft.shell"
            @agent-clicked="openDialog('agents')"
            @model-clicked="openDialog('models')"
            @variant-clicked="workspace.setVariant(session.id, '').catch(notices.pushError)"
          />
          <span class="hint" :class="{ shown: interruptHint }">{{ t('composer.interruptHint') }}</span>
          <SendButton :busy="session.busy" :can-send="canSend" @click="onSendButton" />
        </div>
        <div class="drop-veil" :class="{ shown: dragDepth > 0 }">{{ t('composer.dropImages') }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.box-wrap {
  position: relative;
}

.box {
  position: relative;
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-large);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  transition:
    height var(--motion-standard) var(--motion-ease),
    background-color var(--motion-standard) var(--motion-ease),
    box-shadow var(--motion-standard) var(--motion-ease);
}

.box:focus-within,
.box.dragging {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.box.shell {
  --send-active: var(--shell-border);
  background: var(--shell-surface);
  box-shadow: inset 0 0 0 1px var(--shell-border);
}

.strip-slot {
  flex: none;
  overflow: hidden;
  transition: height var(--motion-standard) var(--motion-ease);
}

.input {
  flex: none;
  min-width: 0;
  resize: none;
  border: 0;
  outline: 0;
  padding: 5px 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  line-height: 22px;
}

.input::placeholder {
  color: var(--text-muted);
}

.tag-row {
  display: flex;
  flex: none;
  align-items: center;
  gap: var(--composer-tags-hint-gap);
  margin-top: var(--composer-text-gap);
}

.tag-row > :first-child {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}

.hint {
  flex: none;
  max-width: 0; /* takes no room from the tags until it is shown */
  overflow: hidden;
  color: var(--text);
  font-size: var(--small-font-size);
  white-space: nowrap;
  opacity: 0;
  transition:
    opacity var(--motion-quick) var(--motion-ease),
    max-width var(--motion-standard) var(--motion-ease);
}

.hint.shown {
  max-width: 240px;
  opacity: 1;
}

.drop-veil {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: inherit;
  background: color-mix(in srgb, var(--surface) 90%, transparent);
  color: var(--text-muted);
  opacity: 0;
  pointer-events: none;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.drop-veil.shown {
  opacity: 1;
}
</style>
