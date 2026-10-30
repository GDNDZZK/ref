<script setup lang="ts">
// A hand-drawn line icon from assets/icons, e.g. <Icon icon="copy-16" />. It takes the text color
// (currentColor). Shown at its grid size, the 16 set next to 12-13px text and the 20 set in
// buttons; place it on whole CSS pixels so the strokes stay crisp (assets/icons/INDEX.md).
import { icons } from '../icons/generated/icons.ts'
import type { IconKey } from '../icons/generated/icons.ts'

defineProps<{ icon: IconKey }>()
</script>

<template>
  <span
    class="icon"
    :class="icon.endsWith('-20') ? 'size-20' : 'size-16'"
    aria-hidden="true"
    v-html="icons[icon]"
  />
</template>

<style scoped>
.icon {
  display: block;
  flex: none;
}

.size-16 {
  width: 16px;
  height: 16px;
}

.size-20 {
  width: 20px;
  height: 20px;
}

.icon :deep(svg) {
  display: block;
}
</style>
