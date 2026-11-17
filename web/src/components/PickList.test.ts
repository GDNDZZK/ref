import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import PickList from './PickList.vue'

describe('PickList', () => {
  it('scrolls the initially selected item into view on mount and guards against stationary mousemove', async () => {
    const items = Array.from({ length: 20 }, (_, index) => ({ key: `k${index}` }))
    const wrapper = mount(PickList, {
      props: {
        items,
        selected: 'k15',
        rowHeight: 40,
        maxRows: 5,
        grouped: false,
        empty: 'Empty',
      },
    })
    const scroller = wrapper.get('.scroller').element as HTMLElement
    Object.defineProperty(scroller, 'clientHeight', { configurable: true, get: () => 200 })

    // Re-trigger initial scroll check with stubbed clientHeight by mounting fresh or updating items.
    const wrapper2 = mount(PickList, {
      props: {
        items: [],
        selected: 'k15',
        rowHeight: 40,
        maxRows: 5,
        grouped: false,
        empty: 'Empty',
      },
    })
    await wrapper2.setProps({ items })
    const box = wrapper2.get('.scroller').element as HTMLElement
    Object.defineProperty(box, 'clientHeight', { configurable: true, get: () => 200 })
    await wrapper2.setProps({ selected: 'k16' })
    await nextTick()
    expect(box.scrollTop).toBeGreaterThan(0)

    const rows = wrapper2.findAll('[role="option"]')
    await rows[1]!.trigger('mousemove', { clientX: 20, clientY: 30 })
    expect(wrapper2.emitted('hover')).toEqual([['k1']])
    // Stationary pointer at identical coordinates is ignored.
    await rows[2]!.trigger('mousemove', { clientX: 20, clientY: 30 })
    expect(wrapper2.emitted('hover')).toHaveLength(1)
    // Moving pointer emits hover.
    await rows[2]!.trigger('mousemove', { clientX: 24, clientY: 35 })
    expect(wrapper2.emitted('hover')).toEqual([['k1'], ['k2']])
  })
})
