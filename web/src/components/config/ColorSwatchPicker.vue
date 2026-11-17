<script setup lang="ts">
// Agent color picker supporting named theme swatches (`var(--agent-*)`), custom `#rrggbb` / `#rgb`
// hex input with live preview, and clearing back to automatic palette selection (`""`).
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import TextButton from '../TextButton.vue'

const SWATCH_NAMES = ['orange', 'blue', 'purple', 'red', 'yellow', 'green', 'cyan'] as const
type SwatchName = (typeof SWATCH_NAMES)[number]

const HEX_COLOR = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i

withDefaults(defineProps<{ disabled?: boolean; label?: string }>(), {
  disabled: false,
  label: undefined,
})

const color = defineModel<string>({ required: true })
const { t } = useI18n()

const swatches = computed(() =>
  SWATCH_NAMES.map((name) => ({
    name,
    cssVar: `var(--agent-${name})`,
    label: t(`config.color.${name}`),
  })),
)

const normalized = computed(() => color.value.trim().toLowerCase())
const customHexPreview = computed(() => (HEX_COLOR.test(normalized.value) ? normalized.value : null))

function selectSwatch(name: SwatchName): void {
  color.value = normalized.value === name ? '' : name
}
</script>

<template>
  <div class="color-picker" :class="{ disabled }">
    <div class="swatches" role="radiogroup" :aria-label="label">
      <button
        v-for="swatch in swatches"
        :key="swatch.name"
        type="button"
        role="radio"
        class="swatch"
        :class="{ active: normalized === swatch.name }"
        :style="{ background: swatch.cssVar }"
        :aria-label="swatch.label"
        :aria-checked="normalized === swatch.name"
        :title="swatch.label"
        :disabled="disabled"
        @click="selectSwatch(swatch.name)"
      />
    </div>

    <div class="hex-wrap">
      <span
        v-if="customHexPreview"
        class="hex-preview"
        :style="{ background: customHexPreview }"
        aria-hidden="true"
      />
      <input
        v-model="color"
        type="text"
        class="hex-input"
        :class="{ 'with-preview': customHexPreview }"
        spellcheck="false"
        autocomplete="off"
        :disabled="disabled"
        :placeholder="t('config.color.hexPlaceholder')"
        :aria-label="label ?? t('config.color.hexPlaceholder')"
      />
    </div>

    <TextButton v-if="color !== ''" :label="t('config.clear')" @click="!disabled && (color = '')" />
    <span v-else class="auto-hint">{{ t('config.color.default') }}</span>
  </div>
</template>

<style scoped>
.color-picker {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
}

.color-picker.disabled {
  opacity: var(--disabled-opacity);
  pointer-events: none;
}

.swatches {
  display: flex;
  align-items: center;
  gap: 6px;
}

.swatch {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.15);
  transition:
    scale var(--motion-quick) var(--motion-ease),
    box-shadow var(--motion-quick) var(--motion-ease);
}

.swatch:not(:disabled):hover {
  scale: 1.08;
}

.swatch.active {
  box-shadow:
    0 0 0 2px var(--background),
    0 0 0 4px var(--text);
}

.hex-wrap {
  position: relative;
  display: flex;
  align-items: center;
}

.hex-preview {
  position: absolute;
  left: 10px;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.2);
  pointer-events: none;
}

.hex-input {
  width: 112px;
  height: var(--field-height);
  padding: 0 10px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.hex-input.with-preview {
  padding-left: 28px;
}

.hex-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.auto-hint {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}
</style>
