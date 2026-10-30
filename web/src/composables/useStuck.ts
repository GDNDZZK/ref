// Whether a block's sticky header is pinned to the top of the view: true once the block's top
// edge (a one-pixel sentinel) has scrolled out above the view.

import { onBeforeUnmount, onMounted, ref } from 'vue'
import type { Ref } from 'vue'
import { useChatScroll } from './chatScroll.ts'

export function useStuck(sentinel: Readonly<Ref<HTMLElement | null>>): Readonly<Ref<boolean>> {
  const { scroller } = useChatScroll()
  const stuck = ref(false)
  let observer: IntersectionObserver | undefined

  onMounted(() => {
    if (!sentinel.value) return
    observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry) return
        const viewTop = entry.rootBounds?.top ?? 0
        stuck.value = !entry.isIntersecting && entry.boundingClientRect.top < viewTop
      },
      { root: scroller.value },
    )
    observer.observe(sentinel.value)
  })
  onBeforeUnmount(() => observer?.disconnect())

  return stuck
}
