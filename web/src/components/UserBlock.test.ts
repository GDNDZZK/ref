import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { describe, expect, it } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import type { UserBlock as UserBlockModel } from '../model/blocks.ts'
import UserBlock from './UserBlock.vue'

function makeBlock(overrides: Partial<UserBlockModel> = {}): UserBlockModel {
  return {
    kind: 'user',
    key: 'm1:0',
    messageId: 'm1',
    streaming: false,
    stale: false,
    text: 'Check [Image 1] and [Image 2] please',
    source: 'human',
    senderName: undefined,
    opensTurn: false,
    images: [],
    ...overrides,
  }
}

describe('UserBlock', () => {
  it('highlights [Image N] tokens with .image-token span and preserves plain text without v-html', () => {
    const wrapper = mount(UserBlock, {
      props: {
        block: makeBlock({
          text: 'Compare [Image 1] with [Image 2], ignore [image 3] and [Image ] <tag>',
        }),
        fresh: false,
      },
      global: { plugins: [createPinia(), createAppI18n(['en'])] },
    })

    const textEl = wrapper.get('.text')
    const tokens = textEl.findAll('span.image-token')
    expect(tokens.map((token) => token.text())).toEqual(['[Image 1]', '[Image 2]'])
    expect(textEl.text()).toBe('Compare [Image 1] with [Image 2], ignore [image 3] and [Image ] <tag>')
    expect(textEl.find('tag').exists()).toBe(false)

    // Verify font-color-only styling (no background property in .image-token rule).
    const source = readFileSync(resolve(process.cwd(), 'src/components/UserBlock.vue'), 'utf8')
    expect(source).toMatch(/\.image-token\s*\{\s*color:\s*var\(--accent-text\);\s*\}/)
  })
  it('keeps shell user blocks using markdown prose rendering', () => {
    const wrapper = mount(UserBlock, {
      props: {
        block: makeBlock({ source: 'shell', text: '`echo [Image 1]`' }),
        fresh: false,
      },
      global: { plugins: [createPinia(), createAppI18n(['en'])] },
    })

    expect(wrapper.find('.text.shell.prose').exists()).toBe(true)
    expect(wrapper.find('.image-token').exists()).toBe(false)
  })
})
