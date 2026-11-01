<script setup lang="ts">
// Asks for the key when ref-web needs one: the first visit without a key in the address, a key
// that ref-web changed, or a salt it does not know. Replaces the whole app until a key is accepted.
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConnectionStore } from '../stores/connection.ts'
import DialogButton from './DialogButton.vue'
import TextField from './TextField.vue'

// Where ref-web keeps the default key (crates/web/src/token.rs).
const TOKEN_FILE = '<state dir>/web/login-token'

const { t } = useI18n()
const connection = useConnectionStore()

const key = ref('')
const checking = ref(false)
const rejected = ref(false)
const field = useTemplateRef<InstanceType<typeof TextField>>('field')

const canSubmit = computed(() => key.value !== '' && !checking.value)

async function submit(): Promise<void> {
  if (!canSubmit.value) return
  checking.value = true
  rejected.value = false
  const accepted = await connection.submitKey(key.value)
  checking.value = false
  rejected.value = !accepted
  if (!accepted) field.value?.select()
}

onMounted(() => field.value?.focus())
</script>

<template>
  <main class="key-entry">
    <form class="card" @submit.prevent="submit">
      <h1 class="title">{{ t('key.title') }}</h1>
      <TextField
        ref="field"
        v-model="key"
        secret
        :label="t('key.label')"
        :error="rejected ? t('key.rejected') : undefined"
        @enter="submit"
      />
      <p v-if="checking" class="status" role="status">{{ t('key.checking') }}</p>
      <p class="hint">{{ t('key.hint', { file: TOKEN_FILE }) }}</p>
      <div class="actions">
        <DialogButton :label="t('key.signIn')" kind="primary" :disabled="!canSubmit" @click="submit" />
      </div>
    </form>
  </main>
</template>

<style scoped>
.key-entry {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  padding: 12px;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: min(var(--dialog-width), 100%);
  padding: 22px;
  border-radius: var(--radius-large);
  background: var(--surface);
  box-shadow:
    inset 0 0 0 1px var(--border),
    0 12px 40px #00000040;
}

.title {
  margin: 0 0 4px;
  font-size: calc(var(--font-size) + 1px);
  font-weight: 700;
}

.status,
.hint {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 8px;
}
</style>
