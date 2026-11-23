import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
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
import { useConnectionStore } from '../stores/connection.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import Composer from './Composer.vue'
import { useWorkspaceStore } from '../stores/workspace.ts'

const COMMANDS: SlashCommand[] = [
  { name: 'undo', aliases: [], title: 'Undo the last prompt', requiresArgs: false, custom: false },
  { name: 'rename', aliases: [], title: 'Rename the session', requiresArgs: true, custom: false },
]

function setup(
  options: {
    busy?: boolean
    queued?: Message[]
    history?: string[]
    attached?: boolean
    extraCommands?: SlashCommand[]
  } = {},
) {
  const api = {
    prompt: vi.fn<ComposerApi['prompt']>(async () => ({ queued: false, message_id: 'm1' })),
    shell: vi.fn<ComposerApi['shell']>(async () => {}),
    removeQueued: vi.fn<ComposerApi['removeQueued']>(async () => ({ removed: false, text: '', images: [] })),
    listFiles: vi.fn<ComposerApi['listFiles']>(async () => []),
    listDirs: vi.fn<NonNullable<ComposerApi['listDirs']>>(async (path) => ({
      path,
      dirs: ['src', 'docs'],
      files: [],
    })),
    listBinaries: vi.fn<NonNullable<ComposerApi['listBinaries']>>(async () => ['cargo', 'cargo-fmt']),
    completeShell: vi.fn<NonNullable<ComposerApi['completeShell']>>(async () => []),
    expandCommand: vi.fn<ComposerApi['expandCommand']>(async () => ''),
  }
  const host: ComposerHost = {
    sessionId: 's1',
    api: () => api,
    history: { history: new PromptHistory(options.history ?? []), push: () => {} },
    cwd: () => '/work',
    isBusy: () => options.busy ?? false,
    hasDelayedTool: () => false,
    slashCommands: () => [...COMMANDS, ...(options.extraCommands ?? [])],
    runSlash: vi.fn(async () => {}),
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
  const pinia = createPinia()
  const wrapper = mount(Composer, {
    props: { session, composer },
    global: { plugins: [pinia, createAppI18n(['en'])] },
    attachTo: options.attached ? document.body : undefined, // focus needs the box in the document
  })
  return { wrapper, api, host, composer, pinia }
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
  it('takes the focus when the composer is asked to', async () => {
    const { wrapper, composer } = setup({ attached: true })
    expect(document.activeElement).not.toBe(wrapper.get('textarea').element)
    composer.requestFocus()
    await flushPromises()
    expect(document.activeElement).toBe(wrapper.get('textarea').element)
    wrapper.unmount()
  })

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

  it('keeps arrow-key selection across keyup when a non-empty slash query is typed', async () => {
    const { wrapper, host } = setup()
    const run = vi.spyOn(host, 'runSlash')
    await type(wrapper, '/n')
    await wrapper.get('textarea').trigger('input')
    expect(wrapper.findAll('[role=option]').map((row) => row.text())).toEqual([
      '/undoUndo the last prompt',
      '/renameRename the session',
    ])
    await press(wrapper, 'ArrowDown')
    await wrapper.get('textarea').trigger('keyup', { key: 'ArrowDown' })
    await flushPromises()
    expect(wrapper.findAll('[role=option]')[1]?.attributes('aria-selected')).toBe('true')
    await press(wrapper, 'Tab')
    expect(value(wrapper)).toBe('/rename ')
    expect(run).not.toHaveBeenCalled()
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

  it('drops unsupported files and pastes file:// URLs as paths at the caret', async () => {
    const { wrapper } = setup()
    const doc = new File(['hello'], 'readme.md', { type: 'text/markdown' })
    await wrapper.get('.box').trigger('drop', {
      dataTransfer: {
        types: ['Files', 'text/uri-list'],
        files: [doc],
        getData: (kind: string) => (kind === 'text/uri-list' ? 'file:///work/readme.md' : ''),
      },
    })
    await vi.waitFor(() => expect(value(wrapper)).toBe('/work/readme.md '))

    await wrapper.get('textarea').trigger('paste', {
      clipboardData: {
        files: [],
        getData: (kind: string) => (kind === 'text/plain' ? 'file:///work/main.rs' : ''),
      },
    })
    await vi.waitFor(() => expect(value(wrapper)).toBe('/work/readme.md /work/main.rs '))
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

  it('opens VariantsDialog when clicking the variant pill', async () => {
    const { wrapper, pinia } = setup()
    const connection = useConnectionStore(pinia)
    const dialogs = useDialogsStore(pinia)
    connection.config = {
      agents: [],
      models: [
        {
          provider: 'anthropic',
          provider_name: 'Anthropic',
          id: 'claude-sonnet-4-5',
          name: 'Claude Sonnet 4.5',
          context: 200000,
          images: true,
          reasoning: true,
          variants: ['low', 'high'],
        },
      ],
      commands: [],
    } as never
    await flushPromises()
    const pills = wrapper.findAll('.tags button')
    await pills[2]!.trigger('click')
    expect(dialogs.current).toEqual({ kind: 'variants', sessionId: 's1' })
  })

  it('never cycles agent on Tab in shell mode and enforces free-form Enter on shell and /cd completions', async () => {
    vi.useFakeTimers()
    const { wrapper, api, host, pinia } = setup({
      extraCommands: [
        { name: 'cd', aliases: [], title: 'Change directory', requiresArgs: false, custom: false },
      ],
    })
    const workspace = useWorkspaceStore(pinia)
    const cycleSpy = vi.spyOn(workspace, 'cycleAgent').mockResolvedValue(undefined)
    const runSlash = vi.spyOn(host, 'runSlash')

    // Enter shell mode: Tab never cycles agent.
    await press(wrapper, '!')
    await press(wrapper, 'Tab')
    await press(wrapper, 'Tab', { shiftKey: true })
    expect(cycleSpy).not.toHaveBeenCalled()

    // Typing in shell mode opens binary completions; plain Enter without ↑/↓ submits typed command directly!
    await type(wrapper, 'car')
    await wrapper.get('textarea').trigger('input')
    await vi.advanceTimersByTimeAsync(100)
    expect(wrapper.findAll('[role=option]').length).toBeGreaterThan(0)
    await press(wrapper, 'Enter')
    expect(api.shell).toHaveBeenCalledWith('s1', 'car')

    // With ↑/↓ (explicitlySelected === true), Enter accepts the selected completion instead of submitting.
    await press(wrapper, '!')
    await type(wrapper, 'car')
    await wrapper.get('textarea').trigger('input')
    await vi.advanceTimersByTimeAsync(100)
    await press(wrapper, 'ArrowDown')
    await press(wrapper, 'Enter')
    expect(value(wrapper)).toBe('cargo-fmt ')
    expect(api.shell).toHaveBeenCalledTimes(1)
    await press(wrapper, 'Escape')
    await press(wrapper, 'Escape')

    // In `/cd `, Tab drills down directories while plain Enter without ↑/↓ submits `/cd s` directly.
    await type(wrapper, '/cd s')
    await wrapper.get('textarea').trigger('input')
    await vi.advanceTimersByTimeAsync(100)
    expect(wrapper.findAll('[role=option]').map((r) => r.text())).toEqual(['src/'])
    await press(wrapper, 'Enter')
    expect(runSlash).toHaveBeenCalledWith('/cd s')
    vi.useRealTimers()
  })

  it('handles standalone-capable slash completion on Tab vs Enter and mid-text slash completion', async () => {
    const { wrapper, host, api } = setup({
      extraCommands: [
        { name: 'sessions', aliases: [], title: 'Sessions', requiresArgs: false, custom: false },
      ],
    })
    const runSlash = vi.spyOn(host, 'runSlash')

    await type(wrapper, '/ses')
    await wrapper.get('textarea').trigger('input')
    await press(wrapper, 'Tab')
    await wrapper.get('textarea').trigger('keyup', { key: 'Tab' })
    expect(value(wrapper)).toBe('/sessions')
    expect(wrapper.find('[role=option]').exists()).toBe(false)
    expect(runSlash).not.toHaveBeenCalled()

    await type(wrapper, '/ses')
    await wrapper.get('textarea').trigger('input')
    await press(wrapper, 'Enter')
    expect(runSlash).toHaveBeenCalledWith('/sessions')
    expect(value(wrapper)).toBe('')

    // Mid-text `/un` completes in place on Enter without submitting, then the next Enter sends.
    await type(wrapper, 'explain /un')
    await wrapper.get('textarea').trigger('input')
    expect(wrapper.findAll('[role=option]').map((r) => r.text())).toEqual(['/undoUndo the last prompt'])
    await press(wrapper, 'Enter')
    expect(value(wrapper)).toBe('explain /undo')
    expect(api.prompt).not.toHaveBeenCalled()
    await press(wrapper, 'Enter')
    expect(api.prompt).toHaveBeenCalledWith('s1', 'explain /undo', [])
  })

  it('completes binaries after shell wrappers without matching full-line history', async () => {
    vi.useFakeTimers()
    const { wrapper, api } = setup({ history: ['!cargo test'] })
    await press(wrapper, '!')
    await type(wrapper, 'sudo car')
    await wrapper.get('textarea').trigger('input')
    await vi.advanceTimersByTimeAsync(100)
    expect(api.listBinaries).toHaveBeenCalledWith('car', 50)
    expect(wrapper.findAll('[role=option]').map((r) => r.text())).toEqual(['cargo', 'cargo-fmt'])
    await press(wrapper, 'Tab')
    expect(value(wrapper)).toBe('sudo cargo ')
    vi.useRealTimers()
  })
  it('triggers immediate 0ms shell completion on Tab, renders descriptions, and applies fallback rules', async () => {
    vi.useFakeTimers()
    const { wrapper, api, pinia } = setup()
    const workspace = useWorkspaceStore(pinia)
    const cycleSpy = vi.spyOn(workspace, 'cycleAgent').mockResolvedValue(undefined)
    api.completeShell.mockImplementation(async (_cwd, line) => {
      if (line === 'docker ') {
        return [
          { value: 'compose', description: 'Compose apps' },
          { value: '--format=', description: 'Format' },
        ]
      }
      return []
    })

    // Empty `!` + Tab opens binaries immediately at 0ms without cycling agent.
    await press(wrapper, '!')
    await press(wrapper, 'Tab')
    expect(cycleSpy).not.toHaveBeenCalled()
    expect(api.listBinaries).toHaveBeenCalledWith('', 50)
    expect(wrapper.findAll('[role=option]').map((r) => r.text())).toEqual(['cargo', 'cargo-fmt'])
    await press(wrapper, 'Escape')

    // `docker ` + Tab opens completeShell items at 0ms with descriptions.
    await type(wrapper, 'docker ')
    await press(wrapper, 'Tab')
    expect(api.completeShell).toHaveBeenCalledWith('/work', 'docker ', { cursor: 7, limit: 50 })
    expect(wrapper.findAll('[role=option]').map((r) => r.text())).toEqual([
      'composeCompose apps',
      '--format=Format',
    ])

    // Plain Enter without ↑/↓ submits free-form instead of inserting completion.
    await press(wrapper, 'Enter')
    expect(api.shell).toHaveBeenCalledWith('s1', 'docker')

    // Re-enter shell mode: `=` completion omits trailing space on Tab.
    await press(wrapper, '!')
    await type(wrapper, 'docker ')
    await press(wrapper, 'Tab')
    await press(wrapper, 'ArrowDown')
    await press(wrapper, 'Tab')
    expect(value(wrapper)).toBe('docker --format=')

    // Empty completeShell on `-`-prefixed arg closes without calling listDirs.
    await type(wrapper, 'docker --nope')
    await wrapper.get('textarea').trigger('input')
    await vi.advanceTimersByTimeAsync(100)
    expect(wrapper.find('[role=option]').exists()).toBe(false)
    expect(api.listDirs).not.toHaveBeenCalled()

    // Empty completeShell on non-`-` arg falls back to listDirs.
    await type(wrapper, 'cat s')
    await wrapper.get('textarea').trigger('input')
    await vi.advanceTimersByTimeAsync(100)
    expect(api.listDirs).toHaveBeenCalledWith('.', { cwd: '/work', includeFiles: true })
    expect(wrapper.findAll('[role=option]').map((r) => r.text())).toEqual(['src/'])
    vi.useRealTimers()
  })

  it('renders synchronized font-color-only backdrop for [Image N] tokens and omits it when absent', async () => {
    const { wrapper } = setup()
    const area = wrapper.get('textarea')
    expect(wrapper.find('.input-backdrop').exists()).toBe(false)
    expect(area.classes()).not.toContain('has-image-tokens')

    await type(wrapper, 'plain text without tokens')
    expect(wrapper.find('.input-backdrop').exists()).toBe(false)
    expect(area.classes()).not.toContain('has-image-tokens')

    await type(wrapper, 'Look at [Image 1] and [Image 2]\n')
    await flushPromises()
    const backdrop = wrapper.get('.input-backdrop')
    expect(backdrop.attributes('aria-hidden')).toBe('true')
    expect(area.classes()).toContain('has-image-tokens')
    expect(backdrop.findAll('span.image-token').map((el) => el.text())).toEqual(['[Image 1]', '[Image 2]'])
    expect(backdrop.text()).toContain('Look at [Image 1] and [Image 2]')

    ;(area.element as HTMLTextAreaElement).scrollTop = 42
    await area.trigger('scroll')
    expect((backdrop.element as HTMLElement).scrollTop).toBe(42)

    await type(wrapper, 'cleared')
    await flushPromises()
    expect(wrapper.find('.input-backdrop').exists()).toBe(false)
    expect(area.classes()).not.toContain('has-image-tokens')
    const source = readFileSync(resolve(process.cwd(), 'src/components/Composer.vue'), 'utf8')
    expect(source).toMatch(/\.input-backdrop\s+\.image-token\s*\{\s*color:\s*var\(--accent-text\);\s*\}/)
  })
})
