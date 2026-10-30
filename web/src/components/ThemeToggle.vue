<script setup lang="ts">
// Sun / moon toggle (ThemeToggle.qml). Rays retract and a "bite" slides over the disc to form
// the moon. Switching themes is rare, so this morph may overshoot.
import { useI18n } from 'vue-i18n'
import { toggleTheme, useTheme } from '../composables/useTheme.ts'
import CircleButton from './CircleButton.vue'

const RAY_COUNT = 8
const RAY_STEP_DEGREES = 360 / RAY_COUNT

const { t } = useI18n()
const { isDark } = useTheme()
</script>

<template>
  <CircleButton
    class="theme-toggle"
    :class="{ dark: isDark }"
    :label="isDark ? t('theme.switchToLight') : t('theme.switchToDark')"
    @click="toggleTheme"
  >
    <span class="glyph">
      <span
        v-for="index in RAY_COUNT"
        :key="index"
        class="spoke"
        :style="{ rotate: `${(index - 1) * RAY_STEP_DEGREES}deg` }"
      >
        <span class="ray" />
      </span>
      <span class="disc" />
      <span class="bite" />
    </span>
  </CircleButton>
</template>

<style scoped>
.theme-toggle {
  --move: var(--motion-slow) var(--motion-emphasis-ease);
  view-transition-name: theme-toggle;
}

.glyph {
  position: relative;
  width: 16px;
  height: 16px;
}

.spoke {
  position: absolute;
  inset: 0;
}

.ray {
  position: absolute;
  left: 7px;
  top: -1px;
  width: 2px;
  height: 4px;
  border-radius: 1px;
  background: var(--text);
  transition:
    translate var(--move),
    opacity var(--motion-standard) var(--motion-ease);
}

.dark .ray {
  translate: 0 7px;
  opacity: 0;
}

.disc {
  position: absolute;
  left: 50%;
  top: 50%;
  width: 8px;
  height: 8px;
  translate: -50% -50%;
  border-radius: 50%;
  background: var(--text);
  transition:
    width var(--move),
    height var(--move);
}

.dark .disc {
  width: 14px;
  height: 14px;
}

/* Painted in the button's own color, so it reads as a cut-out. */
.bite {
  position: absolute;
  left: 0;
  top: 0;
  width: 11px;
  height: 11px;
  border-radius: 50%;
  background: var(--face);
  translate: 13px -9px;
  scale: 0;
  transition:
    translate var(--move),
    scale var(--move);
}

.dark .bite {
  translate: 6px -2px;
  scale: 1;
}
</style>
