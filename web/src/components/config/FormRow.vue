<script setup lang="ts">
// A labeled configuration form row supporting horizontal (compact control) and vertical (wide/tall
// editor) layouts, optional description hint, validation error, and a Reset action.
import { useI18n } from 'vue-i18n'
import TextButton from '../TextButton.vue'

withDefaults(
  defineProps<{
    label: string
    description?: string
    vertical?: boolean
    forId?: string
    canReset?: boolean
    resetLabel?: string
    error?: string
  }>(),
  {
    description: undefined,
    vertical: false,
    forId: undefined,
    canReset: false,
    resetLabel: undefined,
    error: undefined,
  },
)

const emit = defineEmits<{ reset: [] }>()
const { t } = useI18n()
</script>

<template>
  <div class="form-row" :class="{ vertical }">
    <div class="meta">
      <div class="heading">
        <label v-if="forId" class="label" :for="forId">{{ label }}</label>
        <span v-else class="label">{{ label }}</span>
        <div v-if="canReset || $slots.actions" class="actions">
          <slot name="actions" />
          <TextButton v-if="canReset" :label="resetLabel ?? t('config.reset')" @click="emit('reset')" />
        </div>
      </div>
      <p v-if="description" class="description">{{ description }}</p>
    </div>

    <div class="control">
      <slot />
      <p v-if="error" class="error" role="alert">{{ error }}</p>
    </div>
  </div>
</template>

<style scoped>
.form-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 10px 0;
}

.form-row.vertical {
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
}

.meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.label {
  color: var(--text);
  font-size: var(--font-size);
  font-weight: 500;
}

.actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: none;
}

.description {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  line-height: 1.4;
}

.control {
  flex: none;
  min-width: 220px;
  max-width: 100%;
}

.vertical .control {
  width: 100%;
  min-width: 0;
}

.error {
  margin: 4px 0 0;
  color: var(--danger-text);
  font-size: var(--small-font-size);
}
</style>
