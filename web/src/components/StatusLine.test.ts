import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { nextTick, shallowReactive } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import type { Activity, ChatSession } from '../model/chatSession.ts'
import type { ContextUsage } from '../proto/generated'
import StatusLine from './StatusLine.vue'

function mountStatusLine(options: { activity: Activity; error?: string; context?: ContextUsage }) {
  const session = shallowReactive({
    id: 's1',
    activity: options.activity,
    retry:
      options.activity === 'retrying'
        ? { attempt: 2, at: Date.now() + 5000, error: options.error ?? '' }
        : undefined,
    busySince: undefined,
    context: options.context,
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

  it('renders recommend and auto threshold ticks at proportional positions', async () => {
    const { wrapper, session } = mountStatusLine({
      activity: 'idle',
      context: { used: 50_000, max: 200_000, recommend: 120_000, auto: 180_000 },
    })

    expect(wrapper.get('.bar .track .fill').attributes('style')).toContain('width: 25%')
    const recommend = wrapper.get('.bar .tick.recommend')
    const auto = wrapper.get('.bar .tick.auto')
    expect(recommend.attributes('style')).toContain('left: 60%')
    expect(auto.attributes('style')).toContain('left: 90%')
    expect(recommend.attributes('aria-hidden')).toBe('true')
    expect(auto.attributes('aria-hidden')).toBe('true')

    session.context = { used: 190_000, max: 200_000, recommend: 100_000, auto: 160_000 }
    await nextTick()
    expect(wrapper.get('.context').classes()).toContain('danger')
    expect(wrapper.get('.bar .tick.recommend').attributes('style')).toContain('left: 50%')
    expect(wrapper.get('.bar .tick.auto').attributes('style')).toContain('left: 80%')
    wrapper.unmount()
  })

  it('omits threshold ticks when values are zero or >= max and hides bar when max <= 0', async () => {
    const { wrapper, session } = mountStatusLine({
      activity: 'idle',
      context: { used: 10_000, max: 100_000, recommend: 0, auto: 100_000 },
    })
    expect(wrapper.find('.tick.recommend').exists()).toBe(false)
    expect(wrapper.find('.tick.auto').exists()).toBe(false)

    session.context = { used: 10_000, max: 100_000, recommend: 75_000, auto: 120_000 }
    await nextTick()
    expect(wrapper.get('.tick.recommend').attributes('style')).toContain('left: 75%')
    expect(wrapper.find('.tick.auto').exists()).toBe(false)

    session.context = { used: 0, max: 0, recommend: 50, auto: 80 }
    await nextTick()
    expect(wrapper.find('.context').exists()).toBe(false)
    wrapper.unmount()
  })
})
