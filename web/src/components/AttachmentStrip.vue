<script setup lang="ts">
// Thumbnails of the attached images, above the text in the input box.
import { useI18n } from 'vue-i18n'
import type { Attachment } from '../composer/draft.ts'
import { dataUrl } from '../composer/images.ts'
import Icon from './Icon.vue'

defineProps<{ attachments: readonly Attachment[] }>()
defineEmits<{ remove: [token: string] }>()

const { t } = useI18n()
</script>

<template>
  <TransitionGroup tag="div" name="thumb" class="strip">
    <div v-for="attachment in attachments" :key="attachment.token" class="thumb">
      <img :src="dataUrl(attachment.image)" :alt="attachment.name ?? attachment.token" />
      <button
        type="button"
        class="remove"
        :title="attachment.name ?? attachment.token"
        :aria-label="t('composer.attachments.remove', { name: attachment.name ?? attachment.token })"
        @click="$emit('remove', attachment.token)"
      >
        <Icon icon="close-16" class="cross" />
      </button>
    </div>
  </TransitionGroup>
</template>

<style scoped>
.strip {
  --thumb-size: 56px;
  --thumb-gap: 8px;
  --remove-size: 18px;
  display: flex;
  gap: var(--thumb-gap);
  overflow-x: auto;
  padding-bottom: var(--thumb-gap);
}

.thumb {
  position: relative;
  flex: none;
  width: var(--thumb-size);
  height: var(--thumb-size);
  border-radius: var(--radius);
  overflow: hidden;
}

img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.remove {
  position: absolute;
  top: 4px;
  right: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: var(--remove-size);
  height: var(--remove-size);
  border-radius: 50%;
  background: color-mix(in srgb, var(--background) 85%, transparent);
  color: var(--text);
  opacity: 0;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.thumb:hover .remove,
.remove:focus-visible {
  opacity: 1;
}

@media (hover: none) {
  .remove {
    opacity: 1;
  }
}

/* A 16px icon drawn at the 8px cross the design asks for. */
.cross {
  scale: 0.5;
}

.thumb-enter-active,
.thumb-move {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    scale var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease);
}

.thumb-leave-active {
  position: absolute;
  transition:
    opacity var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.thumb-enter-from,
.thumb-leave-to {
  opacity: 0;
  scale: 0.8;
}
</style>
