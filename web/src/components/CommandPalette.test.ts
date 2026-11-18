import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { registerBuiltinCommands } from '../commands/builtin.ts'
import type { Command } from '../commands/types.ts'
import { createAppI18n } from '../i18n/index.ts'
import { info } from '../model/testMessages.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
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
  const workspace = useWorkspaceStore()
  dialogs.open({ kind: 'palette' })
  const wrapper = mount(CommandPalette, { global: { plugins: [pinia, createAppI18n(['en'])] } })
  return { wrapper, commands, dialogs, workspace }
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

  it('asks for the argument of a non-dialog command (e.g. peers-name or custom command)', async () => {
    const { wrapper, commands, dialogs } = mountPalette()
    const review = command('review', {
      title: { text: 'Review changes' },
      group: 'custom',
      source: 'config',
      args: { usage: '[args]' },
    })
    commands.register(review)
    await nextTick()
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(review.run).not.toHaveBeenCalled()
    expect(wrapper.find('.chip').exists()).toBe(true)
    expect(wrapper.find('input').attributes('placeholder')).toBe('[args]')

    await wrapper.find('input').setValue('HEAD~1')
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(review.run).toHaveBeenCalledWith('', 'HEAD~1')
    expect(dialogs.current).toBeUndefined()
  })

  it('goes back to the list on Backspace in an empty argument field and on Esc', async () => {
    const { wrapper, commands, dialogs } = mountPalette()
    commands.register(command('peers-name', { args: { usage: '[name]' } }))
    await nextTick()
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(wrapper.find('.chip').exists()).toBe(true)

    await wrapper.find('input').trigger('keydown', { key: 'Backspace' })
    expect(wrapper.find('.chip').exists()).toBe(false)
    expect(rows(wrapper)).toHaveLength(1)

    // Enter argument prompt again, type text, and press Esc: returns to list instead of closing palette.
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    await wrapper.find('input').setValue('worker')
    expect(dialogs.requestClose('escape'), 'Esc guard keeps palette open').toBeUndefined()
    await nextTick()
    expect(dialogs.current).toEqual({ kind: 'palette' })
    expect(wrapper.find('.chip').exists()).toBe(false)
    expect(rows(wrapper)).toHaveLength(1)

    // Second Esc (now back on list) closes the palette.
    dialogs.requestClose('escape')
    expect(dialogs.current).toBeUndefined()
  })

  it.each([
    ['cd', { kind: 'cd', sessionId: 's1' }],
    ['export', { kind: 'export', sessionId: 's1' }],
    ['rename', { kind: 'rename', sessionId: 's1' }],
    ['variant', { kind: 'variants', sessionId: 's1' }],
    ['close-session', { kind: 'sessions', mode: 'close' }],
  ])('opens dedicated dialog directly when choosing %s from palette', async (commandId, expectedDialog) => {
    const { wrapper, dialogs, workspace } = mountPalette()
    registerBuiltinCommands()
    workspace.onFrame({ kind: 'sessions', sessions: [info({ id: 's1', name: 'main' })] })
    workspace.currentId = 's1'
    await nextTick()

    await wrapper.find('input').setValue(commandId)
    await nextTick()
    const target = rows(wrapper).find((row) => row.text().includes(`/${commandId}`))
    expect(target).toBeDefined()
    await target!.trigger('click')
    await nextTick()
    expect(dialogs.current).toEqual(expectedDialog)
  })

  it('leaves a dialog opened by the command alone', async () => {
    const { wrapper, commands, dialogs } = mountPalette()
    commands.register(command('help', { run: () => dialogs.open({ kind: 'help' }) }))
    await nextTick()
    await wrapper.find('input').trigger('keydown', { key: 'Enter' })
    expect(dialogs.current).toEqual({ kind: 'help' })
  })

  it('shows a clear button while query is non-empty and clears on click', async () => {
    const { wrapper, commands } = mountPalette()
    commands.register(command('new'))
    await nextTick()
    expect(wrapper.find('button.clear').exists()).toBe(false)
    await wrapper.find('input').setValue('zzz')
    expect(wrapper.find('button.clear').exists()).toBe(true)
    await wrapper.find('button.clear').trigger('click')
    expect((wrapper.find('input').element as HTMLInputElement).value).toBe('')
    expect(rows(wrapper)).toHaveLength(1)
  })
})
