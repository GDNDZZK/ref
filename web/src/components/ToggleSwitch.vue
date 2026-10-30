<script setup lang="ts">
// An on/off switch (Switch.qml). The knob is a little wider while pressed.
defineProps<{ label: string; disabled?: boolean }>()
const on = defineModel<boolean>({ required: true })
</script>

<template>
  <button
    type="button"
    role="switch"
    class="switch"
    :class="{ on }"
    :aria-checked="on"
    :aria-label="label"
    :disabled="disabled"
    @click="on = !on"
  >
    <span class="knob" />
  </button>
</template>

<style scoped>
.switch {
  position: relative;
  flex: none;
  width: 38px;
  height: 22px;
  border-radius: 11px;
  background: var(--border);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.switch.on {
  background: var(--accent);
}

.switch:disabled {
  cursor: default;
}

.knob {
  position: absolute;
  left: 3px;
  top: 3px;
  width: 16px;
  height: 16px;
  border-radius: 8px;
  background: var(--text-muted);
  transition:
    translate var(--motion-quick) var(--motion-ease),
    width var(--motion-quick) var(--motion-ease),
    background-color var(--motion-quick) var(--motion-ease);
}

.on .knob {
  translate: 16px 0;
  background: var(--on-accent);
}

.switch:not(:disabled):active .knob {
  width: 19px;
}

.on:not(:disabled):active .knob {
  translate: 13px 0;
}
</style>
