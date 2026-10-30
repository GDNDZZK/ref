import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import type { Command } from '../commands/types.ts'
import { createAppI18n } from '../i18n/index.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import CommandPalette from './CommandPalette.vue'

function command(id: string, overrides: Partial<Command> = {}): Command {
  return {
    id,
    title: { key: `command.${id}.title` },
    group: 'session',
    source: 'builtin',
    slash: true,
    run: vi.fn(),
    ...overrides,
  }
}

function mountPalette() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const commands = useCommandsStore()
  const dialogs = useDialogsStore()
  dialogs.open({ kind: 'palette' })
  const wrapper = mount(CommandPalette, { global: { plugins: [pinia, createAppI18n(['en'])] } })
  return { wrapper, commands, dialogs }
}

const rows = (wrapper: ReturnType<typeof mountPalette>['wrapper']) => wrapper.findAll('[role="option"]')

beforeEach(() => vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} })))

describe('CommandPalette', () => {
  it('lists the commands under their group and dims the unavailable ones', () => {
    const { wrapper, commands } = mountPalette()
    commands.register(command('new', { shortcut: { code: 'KeyN', alt: true } }))
    commands.register(command('models', { enabled: () => false }))
    return nextTick().then(() => {
      expect(wrapper.find('.section').text()).toBe('Session')
      expect(rows(wrapper)).toHaveLength(2)
      expect(rows(wrapper)[0]?.text()).toContain('/new')
      expect(rows(wrapper)[0]?.text()).toContain('Alt')
      expect(rows(wrapper)[1]?.classes()).toContain('disabled')
    })
  })

  it('filters by the typed letters and runs the chosen command', async () => {
    const { wrapper, commands, dialogs } = mountPalette()
    const sessions = command('sessions')
    commands.register(command('new'))
    commands.register(sessions)
    await nextTick()
    await wrapper.find('input').setValue('sessio')
    expect(rows(wrapper)).toHaveLength(2)
    await wrapper.find('input').setValue('sessions')
    expect(rows(wrapper)).toHaveLength(1)
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(sessions.run).toHaveBeenCalled()
    expect(dialogs.current).toBeUndefined()
  })

  it('asks for the argument of a command that takes one', async () => {
    const { wrapper, commands, dialogs } = mountPalette()
    const rename = command('rename', { args: { usage: 'NAME' } })
    commands.register(rename)
    await nextTick()
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(rename.run).not.toHaveBeenCalled()
    expect(wrapper.find('.chip').exists()).toBe(true)
    expect(wrapper.find('input').attributes('placeholder')).toBe('NAME')

    await wrapper.find('input').setValue('Work')
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(rename.run).toHaveBeenCalledWith('', 'Work')
    expect(dialogs.current).toBeUndefined()
  })

  it('goes back to the list on Backspace in an empty argument field', async () => {
    const { wrapper, commands } = mountPalette()
    commands.register(command('rename', { args: { usage: 'NAME' } }))
    await nextTick()
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    await wrapper.find('input').trigger('keydown', { key: 'Backspace' })
    expect(wrapper.find('.chip').exists()).toBe(false)
    expect(rows(wrapper)).toHaveLength(1)
  })

  it('leaves a dialog opened by the command alone', async () => {
    const { wrapper, commands, dialogs } = mountPalette()
    commands.register(command('help', { run: () => dialogs.open({ kind: 'help' }) }))
    await nextTick()
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(dialogs.current).toEqual({ kind: 'help' })
  })
})
