<script setup lang="ts">
// The input box of one session (Composer.qml): queue strip above, then the box with attachments,
// text, the tag row and the send button. The logic is in SessionComposer; this file is keys,
// paste and drop, and layout.
//
//   Enter send · Shift+Enter, Ctrl+J new line · Tab / Shift+Tab agent · Esc Esc interrupt
//   ↑ ↓ history · `/` commands · `@` files · `!` shell · paste or drop images
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { fileUriToPath, parseFileUris } from '../composer/sessionComposer.ts'
import type { SessionComposer } from '../composer/sessionComposer.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { hasImageTokens, splitImageTokens } from '../model/imageTokens.ts'
import { prefersReducedMotion } from '../motion/reducedMotion.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import { components } from '../theme/generated/components.ts'
import { motion } from '../theme/generated/motion.ts'
import AttachmentStrip from './AttachmentStrip.vue'
import CompletionPopup from './CompletionPopup.vue'
import ComposerTags from './ComposerTags.vue'
import QueueStrip from './QueueStrip.vue'
import SendButton from './SendButton.vue'
import { Key } from '../keys.ts'

// `folded`: the input box is folded away (an unfocused split tile); the queue strip stays.
const props = defineProps<{ session: ChatSession; composer: SessionComposer; folded?: boolean }>()
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
// `keyCode` of a key an input method is handling: deprecated, but some browsers give no other signal
const IME_PROCESSING_KEY_CODE = 229

const { t } = useI18n()
const connection = useConnectionStore()
const commands = useCommandsStore()
const workspace = useWorkspaceStore()
const dialogs = useDialogsStore()
const notices = useNoticesStore()

const draft = computed(() => props.composer.draft)
const input = useTemplateRef<HTMLTextAreaElement>('input')
const backdrop = useTemplateRef<HTMLElement>('backdrop')
const root = useTemplateRef<HTMLElement>('root')
const inputHeight = ref(SEND_BUTTON_SIZE)
const dragDepth = ref(0) // drag events of the children come in pairs; the box is hovered while > 0
const boxTransitioning = ref(false)
const clipBox = computed(() => Boolean(props.folded) || boxTransitioning.value)
let foldTimer: ReturnType<typeof setTimeout> | undefined
const canSend = computed(() => draft.value.text.trim() !== '' || draft.value.attachments.length > 0)
const stripHeight = computed(() => (draft.value.attachments.length > 0 ? THUMB_SIZE + THUMB_ROW_GAP : 0))
const boxHeight = computed(
  () => 2 * PADDING + stripHeight.value + inputHeight.value + TEXT_TAG_GAP + TAG_ROW_HEIGHT,
)
const interruptHint = computed(() => props.composer.interruptArmed.value)
const showImageBackdrop = computed(() => hasImageTokens(draft.value.text))
const textSegments = computed(() => (showImageBackdrop.value ? splitImageTokens(draft.value.text) : []))
const placeholder = computed(() => {
  if (connection.state !== 'open') return t('composer.placeholderWaiting')
  if (draft.value.shell) return t('composer.placeholderShell')
  return props.session.busy ? t('composer.placeholderQueue') : t('composer.placeholder')
})

function syncBackdropScroll(): void {
  if (backdrop.value && input.value) {
    backdrop.value.scrollTop = input.value.scrollTop
  }
}

function measure(): void {
  const area = input.value
  if (!area) return
  // Temporarily collapse height to measure scrollHeight, then restore any inline style.
  const applied = area.style.height
  area.style.height = '0'
  const wanted = Math.max(SEND_BUTTON_SIZE, area.scrollHeight)
  area.style.height = applied
  inputHeight.value = Math.min(wanted, MAX_INPUT_HEIGHT)
  syncBackdropScroll()
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
  syncBackdropScroll()
}

watch(() => draft.value.text, showCaret)

function sync(): void {
  const area = input.value
  if (area) props.composer.input(area.value, area.selectionStart)
}

function onInput(): void {
  sync()
  syncBackdropScroll()
  requestAnimationFrame(() => {
    measure()
    syncBackdropScroll()
  })
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

// Keys while the list is open. Tab inserts the choice; Enter accepts unless in free-form mode without ↑/↓.
function onListKey(event: KeyboardEvent): boolean {
  const completion = props.composer.completion
  if (!completion) return false
  const ctrlOnly = event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey
  const up = (isPlain(event) && event.key === Key.ArrowUp) || (ctrlOnly && event.key === 'p')
  const down = (isPlain(event) && event.key === Key.ArrowDown) || (ctrlOnly && event.key === 'n')
  if (up || down) {
    props.composer.moveCompletion(up ? -1 : 1)
    return true
  }

  const isPlainTab = event.key === Key.Tab && isPlain(event)
  const isPlainEnter = event.key === Key.Enter && isPlain(event)
  if (!isPlainTab && !isPlainEnter) return false
  if (isPlainEnter && completion.freeformEnter && !completion.explicitlySelected) return false

  const caret = props.composer.acceptCompletion(isPlainEnter)
  if (caret !== undefined) void showCaret()
  return true
}

// ↑ walks back through the history from an empty box, from the very start of the text, and while
// browsing from the first line; ↓ walks forward from the last line while browsing.
function onHistoryKey(event: KeyboardEvent, area: HTMLTextAreaElement): boolean {
  const isArrow = event.key === Key.ArrowUp || event.key === Key.ArrowDown
  if (!isArrow || !isPlain(event) || area.selectionStart !== area.selectionEnd) return false

  const caret = area.selectionStart
  const browsing = props.composer.browsingHistory
  if (event.key === Key.ArrowUp) {
    const onFirstLine = !area.value.slice(0, caret).includes('\n')
    const atStart = caret === 0
    const canBrowseHistory = area.value === '' || atStart || (browsing && onFirstLine)
    if (!canBrowseHistory) return false
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
  if (event.key !== Key.Escape) return false
  props.composer.escape()
  return true
}

function onEnter(event: KeyboardEvent): boolean {
  if (event.key !== Key.Enter || event.shiftKey || event.altKey) return false
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
  if (event.key !== Key.Tab || hasModifier) return false
  if (draft.value.shell) {
    if (!event.shiftKey) props.composer.triggerShellCompletion()
    return true
  }
  workspace.cycleAgent(props.session.id, event.shiftKey ? -1 : 1).catch(notices.pushError)
  return true
}

// `!` into an empty box starts shell mode; Backspace on the empty box leaves it.
function onShellKey(event: KeyboardEvent): boolean {
  const empty = draft.value.text === ''
  const hasModifier = event.ctrlKey || event.metaKey || event.altKey
  const startsShellMode = event.key === '!' && !hasModifier && empty && !draft.value.shell
  if (startsShellMode) {
    props.composer.enterShell()
    return true
  }
  if (event.key === Key.Backspace && empty && draft.value.shell) {
    props.composer.leaveShell()
    return true
  }
  return false
}

// Ctrl+C without a selection empties the box (with a selection it copies, as everywhere).
function onClear(event: KeyboardEvent, area: HTMLTextAreaElement): boolean {
  const noSelection = area.selectionStart === area.selectionEnd
  const hasContent = draft.value.text !== '' || draft.value.shell
  const isCopyShortcut = event.ctrlKey && event.key === 'c'
  if (!isCopyShortcut || !noSelection || !hasContent) return false
  props.composer.clear()
  return true
}

function onKeydown(event: KeyboardEvent): void {
  const area = input.value
  if (!area || event.isComposing || event.keyCode === IME_PROCESSING_KEY_CODE) return
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

async function attach(files: readonly File[], uriList = ''): Promise<void> {
  sync()
  await props.composer.attachFiles(files, uriList)
  input.value?.focus()
  void showCaret()
}

function isFileUriList(text: string): boolean {
  const lines = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== '' && !line.startsWith('#'))
  return lines.length > 0 && lines.every((line) => fileUriToPath(line) !== undefined)
}

// A pasted file or file URL becomes an image attachment or an inserted path; plain non-file text
// alongside a non-file image (e.g. spreadsheet cells) still wins.
function onPaste(event: ClipboardEvent): void {
  const data = event.clipboardData
  const files = [...(data?.files ?? [])]
  const uriList = data?.getData('text/uri-list') ?? ''
  const plain = data?.getData('text/plain') ?? ''
  const hasUriList = parseFileUris(uriList).length > 0
  const plainIsFileUris = isFileUriList(plain)
  if (files.length > 0 && !hasUriList && !plainIsFileUris && plain.trim() !== '') return
  let uris = ''
  if (hasUriList) uris = uriList
  else if (plainIsFileUris) uris = plain
  if (files.length > 0) {
    event.preventDefault()
    void attach(files, uris)
    return
  }
  if (uris) {
    event.preventDefault()
    void attach([], uris)
  }
}

function hasFiles(event: DragEvent): boolean {
  const types = event.dataTransfer?.types
  return types !== undefined && (types.includes('Files') || types.includes('text/uri-list'))
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
  const uriList = event.dataTransfer?.getData('text/uri-list') ?? ''
  if (files.length > 0 || parseFileUris(uriList).length > 0) void attach(files, uriList)
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
onBeforeUnmount(() => {
  clearFoldTimer()
  resizeObserver.disconnect()
})

function clearFoldTimer(): void {
  if (foldTimer === undefined) return
  clearTimeout(foldTimer)
  foldTimer = undefined
}

function onBoxSlotTransitionEnd(event: TransitionEvent): void {
  if (event.target !== event.currentTarget || event.propertyName !== 'grid-template-rows') return
  clearFoldTimer()
  boxTransitioning.value = false
}

watch(
  () => Boolean(props.folded),
  () => {
    clearFoldTimer()
    boxTransitioning.value = true
    const waitMs = prefersReducedMotion() ? 0 : motion.standard + 30
    foldTimer = setTimeout(() => {
      foldTimer = undefined
      boxTransitioning.value = false
    }, waitMs)
  },
)

watch(
  () => props.composer.focusRequests.value,
  () => input.value?.focus(),
)

defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div class="composer">
    <QueueStrip
      :messages="session.queued"
      :removing="composer.removingQueued"
      :compact-bottom="Boolean(folded)"
      @remove="takeBack"
    />
    <div
      class="box-slot"
      :class="{ folded, clip: clipBox }"
      :inert="folded || undefined"
      @transitionend="onBoxSlotTransitionEnd"
    >
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
          <div class="input-wrap" :style="{ height: `${inputHeight}px`, marginLeft: `${TEXT_LEFT_EXTRA}px` }">
            <div v-if="showImageBackdrop" ref="backdrop" class="input-backdrop" aria-hidden="true">
              <template v-for="(segment, index) in textSegments" :key="index">
                <span v-if="segment.isImageToken" class="image-token">{{ segment.text }}</span>
                <template v-else>{{ segment.text }}</template>
              </template>
              <span v-if="draft.text.endsWith('\n')" aria-hidden="true">&#8203;</span>
            </div>
            <textarea
              ref="input"
              class="input"
              :class="{ 'has-image-tokens': showImageBackdrop }"
              rows="1"
              :value="draft.text"
              :placeholder="placeholder"
              :aria-label="t('composer.label')"
              @input="onInput"
              @scroll="syncBackdropScroll"
              @keydown="onKeydown"
              @keyup="onCaretMoved"
              @click="onCaretMoved"
              @paste="onPaste"
            />
          </div>
          <div class="tag-row" :style="{ height: `${TAG_ROW_HEIGHT}px` }">
            <ComposerTags
              :session="session"
              :shell="draft.shell"
              @agent-clicked="openDialog('agents')"
              @model-clicked="openDialog('models')"
              @variant-clicked="dialogs.open({ kind: 'variants', sessionId: session.id })"
            />
            <span class="hint" :class="{ shown: interruptHint }">{{ t('composer.interruptHint') }}</span>
            <SendButton :busy="session.busy" :can-send="canSend" @click="onSendButton" />
          </div>
          <div class="drop-veil" :class="{ shown: dragDepth > 0 }">{{ t('composer.dropFiles') }}</div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Folds by animating the grid track from the box's height to 0; the box is not clipped, so the
   completion popup can rise out of it. A folded box is hidden once the fold is done. */
.box-slot {
  position: relative;
  z-index: 0;
  display: grid;
  grid-template-rows: 1fr;
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    visibility 0s;
}

.box-slot.clip,
.box-slot.clip .box-wrap {
  overflow: hidden;
}

.box-slot.folded {
  grid-template-rows: 0fr;
  opacity: 0;
  visibility: hidden;
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    visibility 0s linear var(--motion-standard);
}

@media (prefers-reduced-motion: reduce) {
  .box-slot,
  .box-slot.folded {
    transition: none;
  }
}

.box-wrap {
  position: relative;
  min-height: 0;
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
  box-shadow: inset 0 0 0 1px var(--accent-text);
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

.input-wrap {
  position: relative;
  flex: none;
  min-width: 0;
}

.input-backdrop,
.input {
  box-sizing: border-box;
  width: 100%;
  height: 100%;
  padding: 5px 0;
  border: 0;
  font: inherit;
  line-height: 22px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.input-backdrop {
  position: absolute;
  inset: 0;
  overflow-y: auto;
  scrollbar-color: transparent transparent;
  color: var(--text);
  pointer-events: none;
  user-select: none;
}

.input-backdrop .image-token {
  color: var(--accent-text);
}

.input {
  position: relative;
  display: block;
  min-width: 0;
  resize: none;
  outline: 0;
  background: transparent;
  color: var(--text);
}

.input.has-image-tokens {
  color: transparent;
  caret-color: var(--text);
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
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
}

.tag-row > :last-child {
  margin-left: auto;
}

.hint {
  flex: 0 1 auto;
  min-width: 0;
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
