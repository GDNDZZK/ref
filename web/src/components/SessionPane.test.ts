import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { describe, expect, it, vi } from 'vitest'
import { nextTick, reactive } from 'vue'
import { PromptHistory } from '../composer/history.ts'
import { SessionComposer } from '../composer/sessionComposer.ts'
import type { ComposerHost } from '../composer/sessionComposer.ts'
import { createAppI18n } from '../i18n/index.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { info, userMessage } from '../model/testMessages.ts'
import type { Message } from '../proto/generated'
import SessionPane from './SessionPane.vue'

const composers = new Map<string, SessionComposer>()

vi.mock('../stores/composer.ts', () => ({
  useComposerStore: () => ({ of: (id: string) => composers.get(id) }),
}))

function makeComposer(id: string): SessionComposer {
  const host: ComposerHost = {
    sessionId: id,
    api: () => {
      throw new Error('no backend in this test')
    },
    history: { history: new PromptHistory([]), push: () => {} },
    cwd: () => '/work',
    isBusy: () => false,
    hasDelayedTool: () => false,
    slashCommands: () => [],
    runSlash: async () => {},
    abort: async () => {},
    toast: () => {},
    reportError: () => {},
  }
  const composer = new SessionComposer(host)
  composers.set(id, composer)
  return composer
}

function mountPane(id: string, composerShown: boolean, queued: Message[] = []) {
  const composer = makeComposer(id)
  const session = reactive({
    id,
    busy: false,
    queued,
    blocks: [],
    info: info({ id }),
  }) as unknown as ChatSession
  const wrapper = mount(SessionPane, {
    props: { session, composerShown },
    attachTo: document.body,
    global: {
      plugins: [createPinia(), createAppI18n(['en'])],
      stubs: { ChatView: true, StatusLine: { template: '<div class="status-line" />' } },
    },
  })
  return { wrapper, composer }
}

describe('SessionPane composer', () => {
  it('folds the composer away but keeps the status line', async () => {
    const { wrapper } = mountPane('s1', true)
    const slot = wrapper.get('.box-slot')
    expect(slot.classes()).not.toContain('folded')
    expect(slot.attributes('inert')).toBeUndefined()

    await wrapper.setProps({ composerShown: false })
    expect(slot.classes()).toContain('folded')
    expect(slot.attributes('inert')).toBeDefined()
    expect(wrapper.find('.status-line').exists()).toBe(true)
    wrapper.unmount()
  })

  it('keeps the queue strip outside the fold', async () => {
    const { wrapper } = mountPane('s4', false, [userMessage('m1', 'later')])
    const queue = wrapper.get('.queue')
    expect(queue.element.closest('.box-slot')).toBeNull()
    expect(queue.text()).toContain('later')
    expect(wrapper.get('.box-slot').element.contains(wrapper.get('textarea').element)).toBe(true)
    wrapper.unmount()
  })

  it('keeps the draft through a fold and an unfold', async () => {
    const { wrapper, composer } = mountPane('s2', true)
    const area = wrapper.get('textarea')
    await area.setValue('half a thought')

    await wrapper.setProps({ composerShown: false })
    await wrapper.setProps({ composerShown: true })

    expect(wrapper.get('textarea').element).toBe(area.element)
    expect((area.element as HTMLTextAreaElement).value).toBe('half a thought')
    expect(composer.draft.text).toBe('half a thought')
    wrapper.unmount()
  })

  it('moves the caret into the box when it unfolds', async () => {
    const { wrapper } = mountPane('s3', false)
    expect(document.activeElement).not.toBe(wrapper.get('textarea').element)

    await wrapper.setProps({ composerShown: true })
    await nextTick()
    expect(document.activeElement).toBe(wrapper.get('textarea').element)
    wrapper.unmount()
  })
})
