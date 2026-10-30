import { flushPromises, mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'
import type { SlashCommand } from '../composer/completion.ts'
import { PromptHistory } from '../composer/history.ts'
import { SessionComposer } from '../composer/sessionComposer.ts'
import type { ComposerApi, ComposerHost } from '../composer/sessionComposer.ts'
import { createAppI18n } from '../i18n/index.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { info, userMessage } from '../model/testMessages.ts'
import type { Message } from '../proto/generated'
import Composer from './Composer.vue'

const COMMANDS: SlashCommand[] = [
  { name: 'undo', aliases: [], title: 'Undo the last prompt', takesArgs: false, custom: false },
  { name: 'rename', aliases: [], title: 'Rename the session', takesArgs: true, custom: false },
]

function setup(options: { busy?: boolean; queued?: Message[]; history?: string[] } = {}) {
  const api = {
    prompt: vi.fn<ComposerApi['prompt']>(async () => ({ queued: false, message_id: 'm1' })),
    shell: vi.fn<ComposerApi['shell']>(async () => {}),
    removeQueued: vi.fn<ComposerApi['removeQueued']>(async () => ({ removed: false, text: '', images: [] })),
    listFiles: vi.fn<ComposerApi['listFiles']>(async () => []),
    expandCommand: vi.fn<ComposerApi['expandCommand']>(async () => ''),
  }
  const host: ComposerHost = {
    sessionId: 's1',
    api: () => api,
    history: { history: new PromptHistory(options.history ?? []), push: () => {} },
    cwd: () => '/work',
    isBusy: () => options.busy ?? false,
    hasDelayedTool: () => false,
    slashCommands: () => COMMANDS,
    runSlash: async () => {},
    abort: vi.fn(async () => {}),
    toast: () => {},
    reportError: () => {},
  }
  const composer = new SessionComposer(host)
  // The parts of a ChatSession the box reads.
  const session = reactive({
    id: 's1',
    busy: options.busy ?? false,
    queued: options.queued ?? [],
    info: info({ id: 's1' }),
  }) as unknown as ChatSession
  const wrapper = mount(Composer, {
    props: { session, composer },
    global: { plugins: [createPinia(), createAppI18n(['en'])] },
  })
  return { wrapper, api, host, composer }
}

type Wrapper = ReturnType<typeof setup>['wrapper']

async function type(wrapper: Wrapper, text: string) {
  const area = wrapper.get('textarea')
  await area.setValue(text)
  ;(area.element as HTMLTextAreaElement).setSelectionRange(text.length, text.length)
}

async function press(wrapper: Wrapper, key: string, init: KeyboardEventInit = {}) {
  await wrapper.get('textarea').trigger('keydown', { key, ...init })
  await flushPromises()
}

function value(wrapper: Wrapper): string {
  return (wrapper.get('textarea').element as HTMLTextAreaElement).value
}

describe('Composer', () => {
  it('sends on Enter and clears the input', async () => {
    const { wrapper, api } = setup()
    await type(wrapper, 'hello')
    await press(wrapper, 'Enter')
    expect(api.prompt).toHaveBeenCalledWith('s1', 'hello', [])
    expect(wrapper.emitted('submitted')).toHaveLength(1)
    expect(value(wrapper)).toBe('')
  })

  it('keeps the line break on Shift+Enter', async () => {
    const { wrapper, api } = setup()
    await type(wrapper, 'hello')
    await press(wrapper, 'Enter', { shiftKey: true })
    expect(api.prompt).not.toHaveBeenCalled()
  })

  it('does not send while an input method is composing', async () => {
    const { wrapper, api } = setup()
    await type(wrapper, 'ni')
    await press(wrapper, 'Enter', { isComposing: true })
    expect(api.prompt).not.toHaveBeenCalled()
  })

  it('does not send blank text', async () => {
    const { wrapper, api } = setup()
    await type(wrapper, '  \n ')
    await press(wrapper, 'Enter')
    expect(api.prompt).not.toHaveBeenCalled()
    expect(wrapper.emitted('submitted')).toBeUndefined()
  })

  it('stops, not sends, from the button while busy', async () => {
    const { wrapper, api, host } = setup({ busy: true })
    await wrapper.get('button.send').trigger('click')
    expect(host.abort).toHaveBeenCalledTimes(1)
    expect(api.prompt).not.toHaveBeenCalled()
  })

  it('still queues a prompt on Enter while busy', async () => {
    const { wrapper, api } = setup({ busy: true })
    await type(wrapper, 'next')
    await press(wrapper, 'Enter')
    expect(api.prompt).toHaveBeenCalledWith('s1', 'next', [])
  })

  it('starts shell mode with `!` and sends the line as a command', async () => {
    const { wrapper, api } = setup()
    await press(wrapper, '!')
    expect(wrapper.get('.box').classes()).toContain('shell')
    await type(wrapper, 'ls')
    await press(wrapper, 'Enter')
    expect(api.shell).toHaveBeenCalledWith('s1', 'ls')
    expect(wrapper.get('.box').classes()).not.toContain('shell')
  })

  it('leaves shell mode with Backspace on the empty box and with Esc', async () => {
    const { wrapper } = setup()
    await press(wrapper, '!')
    await press(wrapper, 'Backspace')
    expect(wrapper.get('.box').classes()).not.toContain('shell')
    await press(wrapper, '!')
    await press(wrapper, 'Escape')
    expect(wrapper.get('.box').classes()).not.toContain('shell')
  })

  it('walks through the history with the arrow keys', async () => {
    const { wrapper } = setup({ history: ['first', 'second'] })
    await press(wrapper, 'ArrowUp')
    expect(value(wrapper)).toBe('second')
    await press(wrapper, 'ArrowUp')
    expect(value(wrapper)).toBe('first')
    await press(wrapper, 'ArrowDown')
    await press(wrapper, 'ArrowDown')
    expect(value(wrapper)).toBe('')
  })

  it('leaves the arrows to the text in the middle of a draft', async () => {
    const { wrapper } = setup({ history: ['first'] })
    await type(wrapper, 'a draft')
    await press(wrapper, 'ArrowUp')
    expect(value(wrapper)).toBe('a draft')
  })

  it('lists commands for `/` and runs the chosen one with Enter', async () => {
    const { wrapper, host } = setup()
    const run = vi.spyOn(host, 'runSlash')
    await type(wrapper, '/')
    await wrapper.get('textarea').trigger('input')
    expect(wrapper.findAll('[role=option]').map((row) => row.text())).toEqual([
      '/undoUndo the last prompt',
      '/renameRename the session',
    ])
    await press(wrapper, 'Enter')
    expect(run).toHaveBeenCalledWith('/undo')
  })

  it('closes the list with Esc without leaving shell mode or interrupting', async () => {
    const { wrapper, host } = setup({ busy: true })
    await type(wrapper, '/')
    await wrapper.get('textarea').trigger('input')
    await press(wrapper, 'Escape')
    expect(wrapper.find('[role=option]').exists()).toBe(false)
    expect(host.abort).not.toHaveBeenCalled()
  })

  it('attaches a pasted image and shows its thumbnail', async () => {
    const { wrapper, api } = setup()
    const png = new File([new Uint8Array([1, 2, 3])], 'shot.png', { type: 'image/png' })
    await wrapper.get('textarea').trigger('paste', {
      clipboardData: { files: [png], getData: () => '' },
    })
    await vi.waitFor(() => expect(wrapper.findAll('.thumb')).toHaveLength(1))
    expect(value(wrapper)).toBe('[Image 1] ')

    await press(wrapper, 'Enter')
    expect(api.prompt).toHaveBeenCalledWith('s1', '[Image 1] ', [{ mime: 'image/png', data: 'AQID' }])
  })

  it('lists the queued prompts; only yours can be taken back', async () => {
    const queued = [
      userMessage('m1', 'mine'),
      userMessage('m2', 'from afar', { kind: 'peer', from_id: 'x', from_name: 'helper', msg_id: 'q' }),
    ]
    const { wrapper, api } = setup({ busy: true, queued })
    const rows = wrapper.findAll('.queue .row')
    expect(rows.map((row) => row.text())).toEqual(['mineRemove', 'from afarfrom helper'])

    await rows[0]!.get('button.recall').trigger('click')
    expect(api.removeQueued).toHaveBeenCalledWith('s1', 'm1')
    expect(rows[1]!.find('button.recall').exists()).toBe(false)
  })
})
