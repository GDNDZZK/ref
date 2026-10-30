<script setup lang="ts">
// A row of picture thumbnails. The bytes come with GetImage; until then a placeholder shows.
// A click opens the picture in the lightbox.
import { computed, ref } from 'vue'
import { useChatSession } from '../composables/chatSessionContext.ts'
import type { ImageRef } from '../model/blocks.ts'
import { formatBytes } from '../model/format.ts'
import ImageLightbox from './ImageLightbox.vue'
import Icon from './Icon.vue'
import { THUMBNAIL_MAX_HEIGHT, THUMBNAIL_MAX_WIDTH } from './toolLimits.ts'

const props = defineProps<{ images: ImageRef[]; height?: number }>()

const session = useChatSession()
const urls = ref<Record<number, string>>({})
const failed = ref<Record<number, boolean>>({})
const open = ref<number>()
const openUrl = computed(() => (open.value === undefined ? undefined : urls.value[open.value]))

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

function caption(image: ImageRef): string {
  return [image.name ?? image.mime, image.size === null ? '' : formatBytes(image.size)]
    .filter(Boolean)
    .join(' · ')
}
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
      @click="open = index"
    >
      <img v-if="urls[index]" class="picture" :src="urls[index]" :alt="caption(image)" />
      <span v-else class="placeholder">
        <Icon :icon="failed[index] ? 'error-16' : 'image-20'" :class="{ failed: failed[index] }" />
      </span>
      <span v-if="urls[index]" class="hover"><Icon icon="maximize-20" /></span>
    </button>
    <ImageLightbox
      v-if="open !== undefined && openUrl"
      :url="openUrl"
      :caption="caption(images[open]!)"
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
  color: var(--danger);
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
