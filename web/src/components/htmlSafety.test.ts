// Text from the model, from tools and from users must show as text. `v-html` is used for the
// Markdown output, so a script or an event handler in that text must never become an element:
// a script in the page could read the stored key (docs/web.md, "Known limits").

import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import type { Component } from 'vue'
import { chatScrollKey } from '../composables/chatScroll.ts'
import { chatSessionKey } from '../composables/chatSessionContext.ts'
import { createAppI18n } from '../i18n/index.ts'
import { renderMarkdown } from '../markdown/markdown.ts'
import type { CompactionBlock, MarkdownBlock, ReasoningBlock, ToolBlock, UserBlock } from '../model/blocks.ts'
import CompactionBlockView from './CompactionBlock.vue'
import MarkdownBlockView from './MarkdownBlock.vue'
import ReasoningBlockView from './ReasoningBlock.vue'
import ToolBody from './ToolBody.vue'
import UserBlockView from './UserBlock.vue'

declare global {
  interface Window {
    __xss?: number
  }
}

const PAYLOADS = [
  '<script>window.__xss = 1</script>',
  '<img src=x onerror="window.__xss = 1">',
  '<a href="javascript:window.__xss=1" onclick="window.__xss=1">click</a>',
  '<iframe src="https://example.invalid/"></iframe>',
  '<svg onload="window.__xss = 1"><circle/></svg>',
  '[link](javascript:window.__xss=1)',
  '![alt](x" onerror="window.__xss=1)',
  '<div style="background:url(javascript:1)">styled</div>',
  '```html\n<script>window.__xss = 1</script>\n```',
]

// Anything that could run or load something. The app's own icons are SVG (Icon.vue, Chevron.vue).
const DANGEROUS = 'script, iframe, object, embed, style, [onerror], [onclick], [onload], [style]'
const SCRIPT_URL = /^\s*javascript:/i

function expectInert(root: Element, what: string): void {
  expect(root.querySelector(DANGEROUS), what).toBeNull()
  const foreignSvg = [...root.querySelectorAll('svg')].filter(
    (svg) => !svg.classList.contains('chevron') && !svg.parentElement?.classList.contains('icon'),
  )
  expect(foreignSvg, what).toEqual([])
  for (const link of root.querySelectorAll('a[href], img[src]')) {
    const url = link.getAttribute('href') ?? link.getAttribute('src') ?? ''
    expect(SCRIPT_URL.test(url), `${what}: ${url}`).toBe(false)
  }
  expect(window.__xss, what).toBeUndefined()
}

const base = { messageId: 'm', streaming: false, stale: false }

function mountWith(component: Component, props: Record<string, unknown>) {
  const chatScroll = {
    scroller: ref(null),
    hoveredMessageId: ref(''),
    holdHeader: () => {},
    releaseHeader: () => {},
    restHold: () => {},
  }
  return mount(component, {
    props: props as never,
    attachTo: document.body,
    global: {
      plugins: [createPinia(), createAppI18n(['en'])],
      provide: {
        [chatScrollKey as symbol]: chatScroll,
        [chatSessionKey as symbol]: { thinkingExpanded: true },
      },
      stubs: { transition: false },
    },
  })
}

afterEach(() => {
  delete window.__xss
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

describe('rendered text', () => {
  it('the check itself notices a payload that became an element', () => {
    for (const payload of PAYLOADS.slice(0, 5)) {
      const host = document.createElement('div')
      host.innerHTML = payload
      expect(() => expectInert(host, payload), payload).toThrow()
      delete window.__xss
    }
  })

  it('renderMarkdown keeps HTML out', () => {
    for (const payload of PAYLOADS) {
      const host = document.createElement('div')
      host.innerHTML = renderMarkdown(payload)
      expectInert(host, payload)
    }
  })

  it('model text becomes text', () => {
    for (const payload of PAYLOADS) {
      const block: MarkdownBlock = { ...base, key: 'k', kind: 'markdown', text: payload }
      const wrapper = mountWith(MarkdownBlockView, { block })
      expectInert(wrapper.element, payload)
      wrapper.unmount()
    }
  })

  it('shows the angle brackets of a payload as characters', () => {
    const block: MarkdownBlock = { ...base, key: 'k', kind: 'markdown', text: PAYLOADS[0]! }
    const wrapper = mountWith(MarkdownBlockView, { block })
    expect(wrapper.text()).toContain('<script>window.__xss = 1</script>')
  })

  it('reasoning becomes text', () => {
    for (const payload of PAYLOADS) {
      const block: ReasoningBlock = { ...base, key: 'k', kind: 'reasoning', text: payload }
      const wrapper = mountWith(ReasoningBlockView, { block })
      expectInert(wrapper.element, payload)
      wrapper.unmount()
    }
  })

  it('a compaction summary becomes text', async () => {
    for (const payload of PAYLOADS) {
      const block: CompactionBlock = {
        ...base,
        key: 'k',
        kind: 'compaction',
        text: payload,
        time: 0,
        last: false,
      }
      const wrapper = mountWith(CompactionBlockView, { block })
      await wrapper.find('button.pill').trigger('click')
      expectInert(wrapper.element, payload)
      wrapper.unmount()
    }
  })

  it('a user message, typed or from the shell, becomes text', () => {
    for (const source of ['human', 'shell'] as const) {
      for (const payload of PAYLOADS) {
        const block: UserBlock = {
          ...base,
          key: 'k',
          kind: 'user',
          text: payload,
          source,
          senderName: undefined,
          opensTurn: false,
          images: [],
        }
        const wrapper = mountWith(UserBlockView, { block, fresh: false })
        expectInert(wrapper.element, `${source}: ${payload}`)
        wrapper.unmount()
      }
    }
  })

  it('tool output and arguments become text', () => {
    for (const payload of PAYLOADS) {
      const block: ToolBlock = {
        ...base,
        key: 'k',
        kind: 'tool',
        callId: 'c',
        name: 'bash',
        purpose: payload,
        state: 'done',
        input: { command: payload },
        rawInput: '',
        output: payload,
        meta: { exit_code: 0 },
        time: 0,
        timeEnd: 1,
        runAt: null,
        permission: undefined,
        question: undefined,
        images: [],
      }
      const wrapper = mountWith(ToolBody, { block })
      expectInert(wrapper.element, payload)
      wrapper.unmount()
    }
  })
})
