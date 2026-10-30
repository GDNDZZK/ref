// Highlighted tokens for a code block that may still be streaming in.
//
// Highlighting is asynchronous (the grammar loads on first use), and a block that grows is
// re-highlighted at most every THROTTLE_MS. Until the newest text is tokenized, the tokens of an
// earlier prefix are shown followed by the rest as plain text, so the code never flickers.

import { computed, onScopeDispose, shallowRef, watch } from 'vue'
import type { Ref } from 'vue'
import { useNoticesStore } from '../stores/notices.ts'
import { highlight } from './highlighter.ts'
import type { HighlightToken, Highlighted } from './highlighter.ts'

const THROTTLE_MS = 100

const failedLanguages = new Set<string>() // reported once, then shown as plain text

export function useHighlight(
  source: () => { text: string; language: string; streaming: boolean },
): Readonly<Ref<readonly HighlightToken[]>> {
  const notices = useNoticesStore()
  const highlighted = shallowRef<Highlighted>()
  let running = false
  let again = false
  let disposed = false

  async function run(): Promise<void> {
    if (running) {
      again = true
      return
    }
    running = true
    try {
      do {
        again = false
        const { text, language, streaming } = source()
        if (failedLanguages.has(language)) break
        highlighted.value = await highlight(text, language)
        if (again && streaming) await new Promise((resolve) => setTimeout(resolve, THROTTLE_MS))
      } while (again && !disposed)
    } catch (error) {
      failedLanguages.add(source().language)
      notices.pushError(error)
    } finally {
      running = false
    }
  }

  watch(() => [source().text, source().language], run, { immediate: true })
  onScopeDispose(() => (disposed = true))

  return computed(() => {
    const { text } = source()
    const known = highlighted.value
    if (!known || !text.startsWith(known.text)) return [{ content: text, style: {} }]
    const rest = text.slice(known.text.length)
    return rest ? [...known.tokens, { content: rest, style: {} }] : known.tokens
  })
}
