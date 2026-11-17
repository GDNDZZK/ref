<script setup lang="ts">
// A picture fitted to the window over a scrim (ImageBlock.qml). Esc, the scrim or the button close it.
import { onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { containScrimWheel } from '../composables/scrimWheel.ts'
import Icon from './Icon.vue'
import { LIGHTBOX_MARGIN } from './toolLimits.ts'
import { Key } from '../keys.ts'

const props = withDefaults(defineProps<{ url: string; caption: string; open?: boolean }>(), { open: true })
const emit = defineEmits<{ close: [] }>()

const { t } = useI18n()

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== Key.Escape) return
  event.stopPropagation()
  event.preventDefault()
  emit('close')
}

// Capture phase: the composer and the permission keys must not see this Esc.
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) window.addEventListener('keydown', onKeydown, true)
    else window.removeEventListener('keydown', onKeydown, true)
  },
  { immediate: true },
)
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown, true))
</script>

<template>
  <Teleport to="body">
    <Transition name="lightbox" appear>
      <div
        v-if="open && url"
        class="scrim"
        :style="{ '--margin': `${LIGHTBOX_MARGIN}px` }"
        role="dialog"
        @click.self="emit('close')"
        @wheel="containScrimWheel"
      >
        <figure class="figure" @click.self="emit('close')">
          <img class="picture" :src="url" :alt="caption" />
          <figcaption class="caption">{{ caption }}</figcaption>
        </figure>
        <button type="button" class="close" :aria-label="t('tool.closeImage')" @click="emit('close')">
          <Icon icon="close-20" />
        </button>
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
