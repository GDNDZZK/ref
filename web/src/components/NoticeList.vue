<script setup lang="ts">
// The toast stack, top right under the corner buttons: the newest at the bottom, at most four.
import { useNoticesStore } from '../stores/notices.ts'
import NoticeToast from './NoticeToast.vue'

const store = useNoticesStore()
</script>

<template>
  <TransitionGroup tag="div" name="notice" class="notices" role="status">
    <NoticeToast
      v-for="notice in store.notices"
      :key="notice.id"
      :notice="notice"
      @dismiss="store.dismiss(notice.id)"
      @pause="store.pause(notice.id)"
      @resume="store.resume(notice.id)"
    />
  </TransitionGroup>
</template>

<style scoped>
.notices {
  position: fixed;
  top: 62px;
  right: 14px;
  z-index: 10;
  width: min(var(--toast-width), calc(100vw - 28px));
  pointer-events: none;
}

.notice-enter-active,
.notice-leave-active {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease),
    grid-template-rows var(--motion-standard) var(--motion-ease),
    margin-bottom var(--motion-standard) var(--motion-ease);
}

.notice-enter-from {
  opacity: 0;
  translate: var(--motion-toast-slide) 0;
}

.notice-leave-to {
  opacity: 0;
  grid-template-rows: 0fr;
  margin-bottom: 0;
}
</style>
