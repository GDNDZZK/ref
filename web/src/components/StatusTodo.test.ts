import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { createAppI18n } from '../i18n/index.ts'
import type { Todo } from '../proto/generated'
import StatusTodo from './StatusTodo.vue'

const todo = (content: string, status: Todo['status']): Todo => ({ content, status })

const mounted: VueWrapper[] = []

function mountTodo(todos: Todo[] | undefined, sessionId = 's1') {
  const wrapper = mount(StatusTodo, {
    props: { sessionId, todos },
    global: { plugins: [createAppI18n(['en'])] },
    attachTo: document.body,
  })
  mounted.push(wrapper)
  return wrapper
}

const popover = () => document.body.querySelector('.todo-popover')

afterEach(() => {
  mounted.splice(0).forEach((wrapper) => wrapper.unmount())
  document.body.innerHTML = ''
})

describe('StatusTodo summary', () => {
  it('shows the item in progress and done/total', () => {
    const wrapper = mountTodo([
      todo('Write code', 'completed'),
      todo('Fix bug', 'in_progress'),
      todo('Ship', 'pending'),
    ])
    expect(wrapper.get('.current').text()).toBe('Fix bug')
    expect(wrapper.get('.count').text()).toBe('· 1/3')
    expect(wrapper.get('.summary').classes()).not.toContain('complete')
  })

  it('shows the first pending item when nothing is in progress', () => {
    const wrapper = mountTodo([
      todo('Write code', 'completed'),
      todo('Ship', 'pending'),
      todo('Later', 'pending'),
    ])
    expect(wrapper.get('.current').text()).toBe('Ship')
    expect(wrapper.get('.count').text()).toBe('· 1/3')
  })

  it('counts cancelled items in neither number', () => {
    const wrapper = mountTodo([todo('a', 'completed'), todo('b', 'cancelled'), todo('c', 'pending')])
    expect(wrapper.get('.count').text()).toBe('· 1/2')
  })

  it('shows a tick and done/total in the success colour when everything is done', () => {
    const wrapper = mountTodo([todo('a', 'completed'), todo('b', 'cancelled'), todo('c', 'completed')])
    expect(wrapper.find('.current').exists()).toBe(false)
    expect(wrapper.get('.count').text()).toBe('2/2')
    expect(wrapper.get('.summary').classes()).toContain('complete')
  })

  it('squeezes a multi-line item into one line', () => {
    const wrapper = mountTodo([todo('First line\n  second line', 'in_progress')])
    expect(wrapper.get('.current').text()).toBe('First line second line')
  })

  it('is absent without countable items', () => {
    expect(mountTodo(undefined).find('.summary').exists()).toBe(false)
    expect(mountTodo([]).find('.summary').exists()).toBe(false)
    expect(
      mountTodo([todo('a', 'cancelled')])
        .find('.summary')
        .exists(),
    ).toBe(false)
  })
})

describe('StatusTodo popover', () => {
  const todos = [
    todo('Write code', 'completed'),
    todo('Fix bug', 'in_progress'),
    todo('Old idea', 'cancelled'),
  ]

  it('opens on a click with the full list, cancelled items included', async () => {
    const wrapper = mountTodo(todos)
    expect(popover()).toBeNull()
    await wrapper.get('.summary').trigger('click')
    expect(wrapper.get('.summary').attributes('aria-expanded')).toBe('true')
    expect(popover()?.querySelector('.head')?.textContent).toBe('Todo 1/2')
    const rows = [...(popover()?.querySelectorAll('.todo .text') ?? [])].map((row) => row.textContent)
    expect(rows).toEqual(['Write code', 'Fix bug', 'Old idea'])
  })

  it('closes on a second click on the button', async () => {
    const wrapper = mountTodo(todos)
    await wrapper.get('.summary').trigger('click')
    await wrapper.get('.summary').trigger('click')
    expect(popover()).toBeNull()
  })

  it('closes on Esc, and the Esc goes no further', async () => {
    const wrapper = mountTodo(todos)
    await wrapper.get('.summary').trigger('click')
    const reached: string[] = []
    document.body.addEventListener('keydown', () => reached.push('body'))
    const event = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    document.body.dispatchEvent(event)
    await wrapper.vm.$nextTick()
    expect(popover()).toBeNull()
    expect(event.defaultPrevented).toBe(true)
    expect(reached).toEqual([])
  })

  it('closes on a click outside but not on a click inside', async () => {
    const wrapper = mountTodo(todos)
    await wrapper.get('.summary').trigger('click')
    popover()!.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    await wrapper.vm.$nextTick()
    expect(popover()).not.toBeNull()
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    await wrapper.vm.$nextTick()
    expect(popover()).toBeNull()
  })

  it('closes when the session changes', async () => {
    const wrapper = mountTodo(todos)
    await wrapper.get('.summary').trigger('click')
    await wrapper.setProps({ sessionId: 's2' })
    expect(popover()).toBeNull()
  })

  it('updates while it is open', async () => {
    const wrapper = mountTodo(todos)
    await wrapper.get('.summary').trigger('click')
    await wrapper.setProps({
      todos: [todo('Write code', 'completed'), todo('Fix bug', 'completed'), todo('Ship', 'in_progress')],
    })
    expect(popover()?.querySelector('.head')?.textContent).toBe('Todo 2/3')
    expect(popover()?.querySelectorAll('.todo')).toHaveLength(3)
    expect(wrapper.get('.current').text()).toBe('Ship')
  })

  it('closes when the list goes away', async () => {
    const wrapper = mountTodo(todos)
    await wrapper.get('.summary').trigger('click')
    await wrapper.setProps({ todos: [] })
    expect(popover()).toBeNull()
  })
})
