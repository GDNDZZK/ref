<script setup lang="ts">
// The window (Main.qml): conversation above, composer below, theme toggle in the corner.
import { nextTick, onMounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AlertBadge from './components/AlertBadge.vue'
import ChatView from './components/ChatView.vue'
import Chevron from './components/Chevron.vue'
import CircleButton from './components/CircleButton.vue'
import Composer from './components/Composer.vue'
import ConnectionBanner from './components/ConnectionBanner.vue'
import Flourish from './components/Flourish.vue'
import NoticeList from './components/NoticeList.vue'
import ThemeToggle from './components/ThemeToggle.vue'
import { usePermissionKeys } from './composables/usePermissionKeys.ts'
import { useConversationStore } from './stores/conversation.ts'
import { useNoticesStore } from './stores/notices.ts'
import { motion } from './theme/generated/motion.ts'

const ALERT_RING_REACH = 2.5

const { t } = useI18n()
const conversation = useConversationStore()
const notices = useNoticesStore()
usePermissionKeys()

const following = ref(true)
const composer = useTemplateRef<InstanceType<typeof Composer>>('composer')

onMounted(() => composer.value?.focus())

function send(text: string): void {
  following.value = true
  conversation.send(text).catch((error) => notices.pushError(error))
}

function stop(): void {
  conversation.stop().catch((error) => notices.pushError(error))
}

// An error that arrived out of sight puts a badge on the jump button, until the button is gone.
// The badge stays while the button fades out, so it does not pop away first.
const alerting = ref(false)
const alertFlourish = useTemplateRef<InstanceType<typeof Flourish>>('alertFlourish')
const alertBadge = useTemplateRef<HTMLElement>('alertBadge')
let clearAlert: ReturnType<typeof setTimeout> | undefined

async function onErrorOutOfSight(): Promise<void> {
  clearTimeout(clearAlert)
  const wasAlerting = alerting.value
  alerting.value = true
  await nextTick()
  if (!alertBadge.value) return
  // A later error while the badge is shown springs it again.
  if (wasAlerting) alertFlourish.value?.play(alertBadge.value)
  else alertFlourish.value?.enter(alertBadge.value)
}

watch(following, (isFollowing) => {
  if (!isFollowing) return
  clearAlert = setTimeout(() => (alerting.value = false), motion.standard)
})
</script>

<template>
  <div class="app">
    <ConnectionBanner />
    <div class="chat">
      <ChatView
        v-model:following="following"
        :blocks="conversation.blocks"
        :turn-in-progress="conversation.busy"
        :generation="conversation.generation"
        @error-out-of-sight="onErrorOutOfSight"
      />
      <p class="empty" :class="{ hidden: conversation.blocks.length > 0 }">{{ t('chat.empty') }}</p>
    </div>
    <div class="composer-area">
      <div class="column">
        <div class="jump" :class="{ shown: !following }">
          <CircleButton :label="t('chat.jumpToLatest')" @click="following = true">
            <Chevron :size="16" :stroke-width="2" class="down" />
          </CircleButton>
          <span v-if="alerting" class="alert">
            <Flourish ref="alertFlourish" :ring-reach="ALERT_RING_REACH" />
            <span ref="alertBadge" class="alert-dot"><AlertBadge :size="12" /></span>
          </span>
        </div>
        <Composer ref="composer" :busy="conversation.busy" @submitted="send" @stop-requested="stop" />
      </div>
    </div>
    <ThemeToggle class="theme-toggle" />
    <NoticeList />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.chat {
  position: relative;
  flex: 1;
  min-height: 0;
}

.empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  color: var(--text-muted);
  font-size: 22px;
  pointer-events: none;
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.empty.hidden {
  opacity: 0;
}

.composer-area {
  padding: 0 var(--page-padding) var(--page-padding);
}

.column {
  position: relative;
  max-width: var(--content-width);
  margin: 0 auto;
}

/* Stacked above the send button, on its center line. */
.jump {
  --state-color: var(--danger);
  position: absolute;
  right: 7px; /* send button center: composer padding 8 + half its 32px, less half this 34px button */
  bottom: calc(100% + 12px);
  opacity: 0;
  visibility: hidden;
  translate: 0 10px;
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease),
    visibility 0s linear var(--motion-standard);
}

.jump.shown {
  opacity: 1;
  visibility: visible;
  translate: 0 0;
  transition-delay: 0s;
}

.down {
  rotate: 90deg; /* points down */
  color: var(--text-muted);
}

/* On the button's rim, at its upper right. The background-colored ring cuts it out of the button. */
.alert {
  position: absolute;
  left: 21px;
  top: -3px;
  width: 16px;
  height: 16px;
  pointer-events: none;
}

.alert-dot {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--background);
}

.theme-toggle {
  position: fixed;
  top: 14px;
  right: 14px;
}
</style>
