<script setup lang="ts">
// Smart secret input: keeps `{env:VAR}` and `{file:...}` placeholders visible as plain text with
// live environment status badges, while masking literal secret strings with a Show/Hide toggle.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { extractEnvRefs, isSecretPlaceholder } from '../../config/jsonPatch.ts'

interface EnvBadge {
  name: string
  state: 'set' | 'unset'
  text: string
}

const props = withDefaults(
  defineProps<{
    envStatus?: Readonly<Record<string, boolean>>
    placeholder?: string
    label?: string
    disabled?: boolean
    id?: string
    maskLiteral?: boolean
  }>(),
  {
    envStatus: undefined,
    placeholder: undefined,
    label: undefined,
    disabled: false,
    id: undefined,
    maskLiteral: true,
  },
)

const modelValue = defineModel<string>({ required: true })
const { t } = useI18n()
const revealed = ref(false)

const hasPlaceholder = computed(() => isSecretPlaceholder(modelValue.value))
const canMaskLiteral = computed(
  () => props.maskLiteral && modelValue.value.length > 0 && !hasPlaceholder.value,
)
const inputType = computed(() => (canMaskLiteral.value && !revealed.value ? 'password' : 'text'))

const envBadges = computed<EnvBadge[]>(() => {
  const statusMap = props.envStatus
  if (!statusMap) return []
  const badges: EnvBadge[] = []
  for (const name of extractEnvRefs(modelValue.value)) {
    if (!Object.hasOwn(statusMap, name)) continue
    const isSet = statusMap[name]
    if (isSet === true) {
      badges.push({ name, state: 'set', text: `✓ ${t('config.secret.envSet', { name })}` })
      continue
    }
    if (isSet === false) {
      badges.push({ name, state: 'unset', text: `⚠ ${t('config.secret.envUnset', { name })}` })
    }
  }
  return badges
})
</script>

<template>
  <div class="secret-field" :class="{ disabled }">
    <div class="input-wrap">
      <input
        :id="id"
        v-model="modelValue"
        :type="inputType"
        class="input"
        :class="{ 'with-toggle': canMaskLiteral, mono: hasPlaceholder }"
        spellcheck="false"
        autocomplete="off"
        :disabled="disabled"
        :placeholder="placeholder"
        :aria-label="label"
      />
      <button
        v-if="canMaskLiteral"
        type="button"
        class="toggle"
        :disabled="disabled"
        @click="revealed = !revealed"
      >
        {{ revealed ? t('config.secret.hide') : t('config.secret.show') }}
      </button>
    </div>

    <div v-if="envBadges.length > 0" class="badges">
      <span v-for="badge in envBadges" :key="badge.name" class="badge" :class="badge.state">
        {{ badge.text }}
      </span>
    </div>
  </div>
</template>

<style scoped>
.secret-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
}

.secret-field.disabled {
  opacity: var(--disabled-opacity);
}

.input-wrap {
  position: relative;
  display: flex;
  align-items: center;
  width: 100%;
}

.input {
  width: 100%;
  height: var(--field-height);
  padding: 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.input.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.input.with-toggle {
  padding-right: 64px;
}

.input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.toggle {
  position: absolute;
  right: 6px;
  height: 26px;
  padding: 0 8px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.toggle:not(:disabled):hover {
  background: var(--surface-hover);
  color: var(--text);
}

.badges {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.badge {
  display: inline-flex;
  align-items: center;
  font-family: var(--font-mono);
  font-size: var(--small-font-size);
}

.badge.set {
  color: var(--success-text);
}

.badge.unset {
  color: var(--warning-text);
}
</style>
