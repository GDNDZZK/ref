<script setup lang="ts">
// A picture fitted to the window over a scrim (ImageBlock.qml) with prev/next, zoom, and pan.
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { clamp } from '../clamp.ts'
import { Key } from '../keys.ts'
import Chevron from './Chevron.vue'
import Icon from './Icon.vue'
import { LIGHTBOX_MARGIN } from './toolLimits.ts'

export interface LightboxItem {
  url: string
  caption: string
}

const MIN_SCALE = 0.25
const MAX_SCALE = 5
const BUTTON_ZOOM_STEP = 0.25
const WHEEL_ZOOM_FACTOR = 1.15
const DOUBLE_CLICK_SCALE = 2
const DRAG_THRESHOLD_PX = 3
const PERCENT = 100
const CHEVRON_SIZE = 18

const props = withDefaults(
  defineProps<{
    open?: boolean
    images?: readonly LightboxItem[]
    index?: number
    url?: string
    caption?: string
  }>(),
  { open: true, images: () => [], index: 0, url: '', caption: '' },
)
const emit = defineEmits<{ close: []; 'update:index': [index: number] }>()

const { t } = useI18n()

const localIndex = ref(props.index)
watch(
  () => props.index,
  (next) => {
    localIndex.value = next
  },
)

const items = computed<readonly LightboxItem[]>(() => {
  if (props.images && props.images.length > 0) return props.images
  return props.url ? [{ url: props.url, caption: props.caption }] : []
})
const currentIndex = computed(() =>
  items.value.length === 0 ? 0 : clamp(localIndex.value, 0, items.value.length - 1),
)
const currentItem = computed(() => items.value[currentIndex.value])
const hasMultiple = computed(() => items.value.length > 1)
const canPrev = computed(() => currentIndex.value > 0)
const canNext = computed(() => currentIndex.value < items.value.length - 1)

const scale = ref(1)
const panX = ref(0)
const panY = ref(0)
const dragging = ref(false)
let dragPointerId: number | undefined
let dragStartX = 0
let dragStartY = 0
let startPanX = 0
let startPanY = 0
let didDrag = false
let suppressScrimClick = false
let suppressTimer: ReturnType<typeof setTimeout> | undefined

const zoomPercent = computed(() => Math.round(scale.value * PERCENT))
const pictureStyle = computed(() => ({
  transform: `translate3d(${panX.value}px, ${panY.value}px, 0) scale(${scale.value})`,
}))

function clearSuppressTimer(): void {
  if (suppressTimer === undefined) return
  clearTimeout(suppressTimer)
  suppressTimer = undefined
}

function resetView(): void {
  scale.value = 1
  panX.value = 0
  panY.value = 0
  dragging.value = false
  dragPointerId = undefined
  didDrag = false
}

function setScale(next: number): void {
  const clamped = clamp(Math.round(next * PERCENT) / PERCENT, MIN_SCALE, MAX_SCALE)
  scale.value = clamped
  if (clamped <= 1) {
    panX.value = 0
    panY.value = 0
  }
}

function zoomIn(): void {
  setScale(scale.value + BUTTON_ZOOM_STEP)
}

function zoomOut(): void {
  setScale(scale.value - BUTTON_ZOOM_STEP)
}

function toggleDoubleClickZoom(): void {
  if (scale.value === 1) setScale(DOUBLE_CLICK_SCALE)
  else resetView()
}

function goPrev(): void {
  if (!canPrev.value) return
  const next = currentIndex.value - 1
  localIndex.value = next
  emit('update:index', next)
}

function goNext(): void {
  if (!canNext.value) return
  const next = currentIndex.value + 1
  localIndex.value = next
  emit('update:index', next)
}

function onWheel(event: WheelEvent): void {
  event.preventDefault()
  event.stopPropagation()
  if (event.deltaY === 0) return
  const next = event.deltaY < 0 ? scale.value * WHEEL_ZOOM_FACTOR : scale.value / WHEEL_ZOOM_FACTOR
  setScale(next)
}

function trySetPointerCapture(target: HTMLElement | null, pointerId: number): void {
  if (!target?.setPointerCapture) return
  try {
    target.setPointerCapture(pointerId)
  } catch (error) {
    if (!(error instanceof DOMException && error.name === 'NotFoundError')) throw error
  }
}

function onPicturePointerDown(event: PointerEvent): void {
  if (event.button !== 0 || scale.value <= 1) return
  event.preventDefault()
  dragging.value = true
  dragPointerId = event.pointerId
  dragStartX = event.clientX
  dragStartY = event.clientY
  startPanX = panX.value
  startPanY = panY.value
  didDrag = false
  trySetPointerCapture(event.currentTarget as HTMLElement | null, event.pointerId)
}

function onPicturePointerMove(event: PointerEvent): void {
  if (!dragging.value || event.pointerId !== dragPointerId) return
  const dx = event.clientX - dragStartX
  const dy = event.clientY - dragStartY
  if (Math.hypot(dx, dy) > DRAG_THRESHOLD_PX) didDrag = true
  panX.value = startPanX + dx
  panY.value = startPanY + dy
}

function onPicturePointerEnd(event: PointerEvent): void {
  if (!dragging.value || event.pointerId !== dragPointerId) return
  const target = event.currentTarget as HTMLElement | null
  if (target?.hasPointerCapture?.(event.pointerId)) {
    target.releasePointerCapture(event.pointerId)
  }
  if (didDrag) {
    clearSuppressTimer()
    suppressScrimClick = true
    suppressTimer = setTimeout(() => {
      suppressTimer = undefined
      suppressScrimClick = false
    }, 0)
  }
  dragging.value = false
  dragPointerId = undefined
  didDrag = false
}

function onScrimClick(): void {
  if (suppressScrimClick) {
    clearSuppressTimer()
    suppressScrimClick = false
    return
  }
  emit('close')
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === Key.Escape) {
    event.stopPropagation()
    event.preventDefault()
    emit('close')
    return
  }
  if (event.ctrlKey || event.metaKey || event.altKey) return
  switch (event.key) {
    case Key.ArrowLeft:
      event.stopPropagation()
      event.preventDefault()
      goPrev()
      return
    case Key.ArrowRight:
      event.stopPropagation()
      event.preventDefault()
      goNext()
      return
    case '+':
    case '=':
      event.stopPropagation()
      event.preventDefault()
      zoomIn()
      return
    case '-':
    case '_':
      event.stopPropagation()
      event.preventDefault()
      zoomOut()
      return
    case '0':
      event.stopPropagation()
      event.preventDefault()
      resetView()
  }
}

watch([() => props.open, currentIndex, () => currentItem.value?.url], () => {
  resetView()
})

// Capture phase: the composer and the permission keys must not see these shortcuts while open.
watch(
  () => props.open && currentItem.value !== undefined,
  (isOpen) => {
    if (isOpen) window.addEventListener('keydown', onKeydown, true)
    else window.removeEventListener('keydown', onKeydown, true)
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  clearSuppressTimer()
  window.removeEventListener('keydown', onKeydown, true)
})
</script>

<template>
  <Teleport to="body">
    <Transition name="lightbox" appear>
      <div
        v-if="open && currentItem"
        class="scrim"
        :style="{ '--margin': `${LIGHTBOX_MARGIN}px` }"
        role="dialog"
        @click.self="onScrimClick"
        @wheel="onWheel"
      >
        <div class="toolbar" @click.stop>
          <span v-if="hasMultiple" class="counter">{{ currentIndex + 1 }} / {{ items.length }}</span>
          <button
            type="button"
            class="tool-btn zoom-out"
            :disabled="scale <= MIN_SCALE"
            :aria-label="t('tool.zoomOut')"
            :title="t('tool.zoomOut')"
            @click="zoomOut"
          >
            <Icon icon="minus-16" />
          </button>
          <button
            type="button"
            class="tool-btn zoom-reset"
            :aria-label="t('tool.zoomReset')"
            :title="t('tool.zoomReset')"
            @click="resetView"
          >
            {{ zoomPercent }}%
          </button>
          <button
            type="button"
            class="tool-btn zoom-in"
            :disabled="scale >= MAX_SCALE"
            :aria-label="t('tool.zoomIn')"
            :title="t('tool.zoomIn')"
            @click="zoomIn"
          >
            <Icon icon="plus-16" />
          </button>
        </div>

        <button type="button" class="close" :aria-label="t('tool.closeImage')" @click.stop="emit('close')">
          <Icon icon="close-20" />
        </button>

        <button
          v-if="hasMultiple"
          type="button"
          class="nav prev"
          :disabled="!canPrev"
          :aria-label="t('tool.previousImage')"
          :title="t('tool.previousImage')"
          @click.stop="goPrev"
        >
          <Chevron :size="CHEVRON_SIZE" class="prev-chevron" />
        </button>

        <button
          v-if="hasMultiple"
          type="button"
          class="nav next"
          :disabled="!canNext"
          :aria-label="t('tool.nextImage')"
          :title="t('tool.nextImage')"
          @click.stop="goNext"
        >
          <Chevron :size="CHEVRON_SIZE" />
        </button>

        <figure class="figure" @click.self="onScrimClick">
          <div class="stage" @click.self="onScrimClick">
            <img
              v-if="currentItem.url"
              class="picture"
              :class="{ zoomed: scale > 1, dragging }"
              :src="currentItem.url"
              :alt="currentItem.caption"
              draggable="false"
              :style="pictureStyle"
              @dblclick.stop="toggleDoubleClickZoom"
              @pointerdown="onPicturePointerDown"
              @pointermove="onPicturePointerMove"
              @pointerup="onPicturePointerEnd"
              @pointercancel="onPicturePointerEnd"
              @lostpointercapture="onPicturePointerEnd"
            />
            <div v-else class="loading-placeholder">
              <Icon icon="image-20" />
            </div>
          </div>
          <figcaption class="caption">{{ currentItem.caption }}</figcaption>
        </figure>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: grid;
  place-items: center;
  background: rgb(0 0 0 / 0.72);
  overscroll-behavior: contain;
}

.toolbar {
  position: absolute;
  top: 14px;
  left: 50%;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 6px;
  border-radius: 999px;
  background: rgb(0 0 0 / 0.56);
  box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.14);
  color: white;
  transform: translateX(-50%);
  user-select: none;
}

.counter {
  padding: 0 8px 0 6px;
  border-right: 1px solid rgb(255 255 255 / 0.16);
  margin-right: 2px;
  color: rgb(255 255 255 / 0.85);
  font-size: var(--small-font-size);
  font-variant-numeric: tabular-nums;
}

.tool-btn {
  display: grid;
  place-items: center;
  min-width: 28px;
  height: 26px;
  padding: 0 6px;
  border-radius: 999px;
  color: white;
  font-size: var(--small-font-size);
  font-variant-numeric: tabular-nums;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.tool-btn:hover:not(:disabled) {
  background: rgb(255 255 255 / 0.16);
}

.tool-btn:disabled {
  opacity: 0.35;
  cursor: default;
}

.nav {
  position: absolute;
  top: 50%;
  z-index: 2;
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  border-radius: 50%;
  background: rgb(255 255 255 / 0.14);
  color: white;
  transform: translateY(-50%);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    opacity var(--motion-quick) var(--motion-ease);
}

.nav:hover:not(:disabled) {
  background: rgb(255 255 255 / 0.24);
}

.nav:disabled {
  opacity: 0.28;
  cursor: default;
}

.nav.prev {
  left: 16px;
}

.nav.next {
  right: 16px;
}

.prev-chevron {
  rotate: 180deg;
}

.figure {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  max-width: calc(100vw - 2 * var(--margin));
  max-height: calc(100vh - 2 * var(--margin));
  margin: 0;
}

.stage {
  display: grid;
  place-items: center;
  min-height: 0;
  overflow: visible;
}

.picture {
  min-height: 0;
  max-width: 100%;
  object-fit: contain;
  /* leaves room for the caption and top toolbar */
  max-height: calc(100vh - 2 * var(--margin) - 28px);
  cursor: zoom-in;
  user-select: none;
  transform-origin: center center;
  transition: transform var(--motion-quick) var(--motion-ease);
}

.picture.zoomed {
  cursor: grab;
  touch-action: none;
}

.picture.dragging {
  cursor: grabbing;
  transition: none;
}

.loading-placeholder {
  display: grid;
  place-items: center;
  width: 96px;
  height: 96px;
  border-radius: var(--radius);
  background: rgb(255 255 255 / 0.08);
  color: rgb(255 255 255 / 0.7);
}

.caption {
  color: rgb(255 255 255 / 0.8);
  font-size: var(--small-font-size);
}

.close {
  position: absolute;
  top: 14px;
  right: 14px;
  z-index: 2;
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  background: rgb(255 255 255 / 0.14);
  color: white;
}

.close:hover {
  background: rgb(255 255 255 / 0.24);
}

.lightbox-enter-active {
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.lightbox-leave-active {
  transition: opacity var(--motion-quick) var(--motion-exit-ease);
}

.lightbox-enter-active .figure {
  transition: scale var(--motion-standard) var(--motion-ease);
}

.lightbox-leave-active .figure {
  transition: scale var(--motion-quick) var(--motion-exit-ease);
}

.lightbox-enter-from,
.lightbox-leave-to {
  opacity: 0;
}

.lightbox-enter-from .figure,
.lightbox-leave-to .figure {
  scale: 0.96;
}
</style>
