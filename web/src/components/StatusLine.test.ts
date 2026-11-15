import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { nextTick, shallowReactive } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import type { Activity, ChatSession } from '../model/chatSession.ts'
import StatusLine from './StatusLine.vue'

function mountStatusLine(options: { activity: Activity; error?: string }) {
  const session = shallowReactive({
    id: 's1',
    activity: options.activity,
    retry:
      options.activity === 'retrying'
        ? { attempt: 2, at: Date.now() + 5000, error: options.error ?? '' }
        : undefined,
    busySince: undefined,
    context: undefined,
    info: undefined,
  })
  const wrapper = mount(StatusLine, {
    props: { session: session as unknown as ChatSession },
    global: {
      plugins: [createAppI18n(['en'])],
      stubs: { StatusTodo: true },
    },
  })
  return { wrapper, session }
}

describe('StatusLine', () => {
  it('sets the raw multiline retry error as title on the retrying warning label', () => {
    const rawError = 'upstream 502 Bad Gateway:\n  rate limit exceeded   on endpoint /v1/messages'
    const { wrapper } = mountStatusLine({ activity: 'retrying', error: rawError })

    const label = wrapper.get('.label.warning')
    expect(label.attributes('title')).toBe(rawError)
    expect(label.text()).toContain('upstream 502 Bad Gateway: rate limit exceeded on endpoint /v1/messages')
    wrapper.unmount()
  })

  it('omits title when retry error is empty or when activity is not retrying', async () => {
    const { wrapper, session } = mountStatusLine({ activity: 'retrying', error: '' })
    expect(wrapper.get('.label.warning').attributes('title')).toBeUndefined()

    session.activity = 'waiting_permission'
    session.retry = undefined
    await nextTick()
    expect(wrapper.get('.label.warning').attributes('title')).toBeUndefined()

    session.activity = 'waiting_question'
    await nextTick()
    expect(wrapper.get('.label.warning').attributes('title')).toBeUndefined()

    session.activity = 'thinking'
    await nextTick()
    expect(wrapper.get('.label').attributes('title')).toBeUndefined()

    wrapper.unmount()
  })
})
