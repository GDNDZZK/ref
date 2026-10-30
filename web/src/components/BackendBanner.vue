<script setup lang="ts">
// Shown when the backend speaks an older protocol than this app: offers to restart it. The
// restart stops running sessions, so the button waits until none works. After Shutdown the
// connection reconnects by itself and ref-web starts the backend that is installed now.
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { PROTOCOL_VERSION } from '../proto/generated'
import { rowState } from '../shell/sessionRow.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useNoticesStore } from '../stores/notices.ts'
import DialogButton from './DialogButton.vue'
import Icon from './Icon.vue'

const PROTOCOL_WITHOUT_FIELD = 1
const RECHECK_MS = 3000

const { t } = useI18n()
const connection = useConnectionStore()
const notices = useNoticesStore()

const working = ref(false) // a session is running or waiting
const restarting = ref(false)
let recheck: ReturnType<typeof setInterval> | undefined

// The workspace is not loaded for an old backend, so ask for the sessions here.
async function look(): Promise<void> {
  try {
    const sessions = await connection.getApi().listSessions()
    working.value = sessions.some((info) => rowState(info) !== 'idle' && rowState(info) !== 'error')
  } catch (error) {
    notices.pushError(error)
  }
}

watch(
  () => connection.backendTooOld,
  (tooOld) => {
    clearInterval(recheck)
    if (!tooOld) return void (restarting.value = false)
    void look()
    recheck = setInterval(() => void look(), RECHECK_MS)
  },
  { immediate: true },
)
onBeforeUnmount(() => clearInterval(recheck))

const text = computed(() =>
  restarting.value
    ? t('banner.restarting')
    : t('banner.backendTooOld', {
        have: connection.hello?.protocol ?? PROTOCOL_WITHOUT_FIELD,
        need: PROTOCOL_VERSION,
      }),
)

async function restart(): Promise<void> {
  await look()
  if (working.value) return
  restarting.value = true
  // The connection ends while the backend stops, so the answer may never arrive.
  await connection
    .getApi()
    .shutdown()
    .catch(() => undefined)
}
</script>

<template>
  <Transition name="banner">
    <div v-if="connection.backendTooOld" class="slot">
      <div class="banner" role="alert">
        <div class="bar">
          <Icon icon="warning-16" class="icon" />
          <span class="text">{{ text }}</span>
          <DialogButton
            v-if="!restarting"
            small
            :label="t('banner.restart')"
            :disabled="working"
            :title="working ? t('banner.busyHint') : undefined"
            @click="restart"
          />
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.slot {
  display: grid;
  flex: none;
  grid-template-rows: 1fr;
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease);
}

.banner {
  min-height: 0;
  overflow: hidden;
}

.bar {
  display: flex;
  align-items: center;
  gap: 10px;
  height: var(--banner-height);
  padding: 0 14px;
  background: var(--warning-surface);
  box-shadow: inset 0 -1px 0 var(--warning-border);
  font-size: calc(var(--small-font-size) + 1px);
}

.icon {
  flex: none;
  color: var(--warning);
}

.text {
  flex: 1;
  min-width: 0;
}

.banner-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
  translate: 0 calc(-1 * var(--banner-height));
}
</style>
