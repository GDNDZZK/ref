<script setup lang="ts">
// A row of picture thumbnails. The bytes come with GetImage; until then a placeholder shows.
// A click opens the picture in the session lightbox (or a local fallback when used standalone).
import { computed, ref } from 'vue'
import { useChatSession } from '../composables/chatSessionContext.ts'
import { imageCaption, useImageLightbox } from '../composables/imageLightboxContext.ts'
import type { ImageRef } from '../model/blocks.ts'
import ImageLightbox from './ImageLightbox.vue'
import Icon from './Icon.vue'
import { THUMBNAIL_MAX_HEIGHT, THUMBNAIL_MAX_WIDTH } from './toolLimits.ts'

const props = defineProps<{ images: ImageRef[]; height?: number }>()

const session = useChatSession()
const sessionLightbox = useImageLightbox()
const urls = ref<Record<number, string>>({})
const failed = ref<Record<number, boolean>>({})
const open = ref<number>()
const everOpened = ref(false)

const standaloneImages = computed(() =>
  props.images.flatMap((image, index) => {
    const url = urls.value[index]
    return url ? [{ index, url, caption: imageCaption(image) }] : []
  }),
)
const lightboxItems = computed(() => standaloneImages.value.map(({ url, caption }) => ({ url, caption })))
const activeStandaloneIndex = computed({
  get: () => {
    if (open.value === undefined) return 0
    const found = standaloneImages.value.findIndex((item) => item.index === open.value)
    return found >= 0 ? found : 0
  },
  set: (pos: number) => {
    const target = standaloneImages.value[pos]
    if (target) open.value = target.index
  },
})

function openThumb(index: number): void {
  const image = props.images[index]
  if (!image) return
  if (sessionLightbox) {
    sessionLightbox.open(image)
    return
  }
  everOpened.value = true
  open.value = index
}

props.images.forEach((image, index) => {
  if (image.id === null) {
    failed.value[index] = true
    return
  }
  session.imageUrl(image.id).then(
    (url) => (urls.value[index] = url),
    () => (failed.value[index] = true),
  )
})
</script>

<template>
  <div
    class="thumbs"
    :style="{
      '--max-width': height === undefined ? `${THUMBNAIL_MAX_WIDTH}px` : 'none',
      '--max-height': `${height ?? THUMBNAIL_MAX_HEIGHT}px`,
    }"
  >
    <button
      v-for="(image, index) in images"
      :key="index"
      type="button"
      class="thumb"
      :disabled="urls[index] === undefined"
      @click="openThumb(index)"
    >
      <img v-if="urls[index]" class="picture" :src="urls[index]" :alt="imageCaption(image)" />
      <span v-else class="placeholder">
        <Icon :icon="failed[index] ? 'error-16' : 'image-20'" :class="{ failed: failed[index] }" />
      </span>
      <span v-if="urls[index]" class="hover"><Icon icon="maximize-20" /></span>
    </button>
    <ImageLightbox
      v-if="everOpened && !sessionLightbox"
      v-model:index="activeStandaloneIndex"
      :open="open !== undefined && lightboxItems.length > 0"
      :images="lightboxItems"
      @close="open = undefined"
    />
  </div>
</template>

<style scoped>
.thumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.thumb {
  position: relative;
  overflow: hidden;
  border-radius: 6px;
  box-shadow: inset 0 0 0 1px var(--border);
  line-height: 0;
}

.picture {
  display: block;
  max-width: var(--max-width);
  max-height: var(--max-height);
}

.placeholder {
  display: grid;
  place-items: center;
  width: var(--max-height);
  height: var(--max-height);
  background: var(--surface);
  color: var(--text-muted);
}

.failed {
  color: var(--danger-text);
}

.thumb::after {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 0.12);
  content: '';
  opacity: 0;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.hover {
  position: absolute;
  top: 4px;
  right: 4px;
  z-index: 1;
  color: white;
  opacity: 0;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.thumb:hover::after,
.thumb:hover .hover {
  opacity: 1;
}
</style>
