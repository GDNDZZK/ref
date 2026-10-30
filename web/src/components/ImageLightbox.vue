<script setup lang="ts">
// A picture fitted to the window over a scrim (ImageBlock.qml). Esc, the scrim or the button close it.
import { onBeforeUnmount, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import Icon from './Icon.vue'
import { LIGHTBOX_MARGIN } from './toolLimits.ts'

defineProps<{ url: string; caption: string }>()
const emit = defineEmits<{ close: [] }>()

const { t } = useI18n()

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return
  event.stopPropagation()
  event.preventDefault()
  emit('close')
}

// Capture phase: the composer and the permission keys must not see this Esc.
onMounted(() => window.addEventListener('keydown', onKeydown, true))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown, true))
</script>

<template>
  <Teleport to="body">
    <div
      class="scrim"
      :style="{ '--margin': `${LIGHTBOX_MARGIN}px` }"
      role="dialog"
      @click.self="emit('close')"
    >
      <figure class="figure" @click.self="emit('close')">
        <img class="picture" :src="url" :alt="caption" />
        <figcaption class="caption">{{ caption }}</figcaption>
      </figure>
      <button type="button" class="close" :aria-label="t('tool.closeImage')" @click="emit('close')">
        <Icon icon="close-20" />
      </button>
    </div>
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
  animation: lightbox-in var(--motion-standard) var(--motion-ease);
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

.picture {
  min-height: 0;
  max-width: 100%;
  object-fit: contain;
  /* leaves room for the caption */
  max-height: calc(100vh - 2 * var(--margin) - 28px);
}

.caption {
  color: rgb(255 255 255 / 0.8);
  font-size: var(--small-font-size);
}

.close {
  position: absolute;
  top: 14px;
  right: 14px;
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

@keyframes lightbox-in {
  from {
    opacity: 0;
    scale: 0.96;
  }
}
</style>
