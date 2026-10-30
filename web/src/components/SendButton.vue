<script setup lang="ts">
// Round send button that morphs into stop while a reply is running (SendButton.qml).
// Into stop the glyphs swap with the usual ease-out; back to send, the square turns away
// with the exit ease, so the end of a reply is seen rather than cut.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import Chevron from './Chevron.vue'

const props = defineProps<{ busy: boolean; canSend: boolean }>()
defineEmits<{ click: [] }>()

const { t } = useI18n()
const active = computed(() => props.busy || props.canSend)
</script>

<template>
  <button
    type="button"
    class="send"
    :class="{ busy, active }"
    :aria-label="busy ? t('composer.stop') : t('composer.send')"
    :aria-disabled="!active"
    @click="active && $emit('click')"
  >
    <span class="face">
      <Chevron class="arrow" :size="16" :stroke-width="2" />
      <span class="stop" />
    </span>
  </button>
</template>

<style scoped>
.send {
  width: 32px;
  height: 32px;
  flex: none;
  cursor: default;
}

.send.active {
  cursor: pointer;
}

.face {
  position: relative;
  display: block;
  width: 100%;
  height: 100%;
  border-radius: 50%;
  background: var(--surface-hover);
  color: var(--text-muted);
  transition:
    background-color var(--motion-slow) var(--motion-ease),
    color var(--motion-slow) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

/* Shell mode sets --send-active to its amber. */
.active .face {
  background: var(--send-active, var(--accent));
  color: var(--on-accent);
}

.active:active .face {
  scale: 0.9;
}

.arrow,
.stop {
  position: absolute;
  left: 50%;
  top: 50%;
  transition:
    opacity var(--motion-quick) var(--motion-exit-ease),
    scale var(--motion-quick) var(--motion-exit-ease),
    rotate var(--motion-quick) var(--motion-exit-ease);
}

.arrow {
  translate: -50% -50%;
  rotate: -90deg; /* points up */
}

.stop {
  width: 10px;
  height: 10px;
  margin: -5px 0 0 -5px;
  border-radius: 2px;
  background: var(--on-accent);
  opacity: 0;
  scale: 0.5;
  rotate: -90deg;
}

.busy .arrow,
.busy .stop {
  transition-duration: var(--motion-standard);
  transition-timing-function: var(--motion-ease);
}

.busy .arrow {
  opacity: 0;
  scale: 0.5;
}

.busy .stop {
  opacity: 1;
  scale: 1;
  rotate: 0deg;
}
</style>
