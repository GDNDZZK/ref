import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { NamedImage } from '../proto/generated'
import type { SlashCommand } from './completion.ts'
import { emitDraftRestored } from './draftBus.ts'
import { PromptHistory } from './history.ts'
import { INTERRUPT_ARMED_MS, SessionComposer } from './sessionComposer.ts'
import type { ToastLevel } from '../proto/generated'
import type { ComposerApi, ComposerHost } from './sessionComposer.ts'

const COMMANDS: SlashCommand[] = [
  { name: 'undo', aliases: [], title: 'Undo', requiresArgs: false, custom: false },
  { name: 'rename', aliases: [], title: 'Rename', requiresArgs: true, custom: false },
  { name: 'sessions', aliases: [], title: 'Sessions', requiresArgs: false, custom: false },
  { name: 'review', aliases: [], title: 'Review', requiresArgs: false, custom: true },
]

function setup(options: { busy?: boolean; delayed?: boolean } = {}) {
  const state = { busy: options.busy ?? false, delayed: options.delayed ?? false }
  const pushed: string[] = []
  const api = {
    prompt: vi.fn<ComposerApi['prompt']>(async () => ({ queued: false, message_id: 'm1' })),
    shell: vi.fn<ComposerApi['shell']>(async () => {}),
    removeQueued: vi.fn<ComposerApi['removeQueued']>(async () => ({ removed: true, text: '', images: [] })),
    listFiles: vi.fn<ComposerApi['listFiles']>(async () => []),
    expandCommand: vi.fn<ComposerApi['expandCommand']>(async () => 'expanded text'),
    completeShell: vi.fn<NonNullable<ComposerApi['completeShell']>>(async () => []),
  }
  const toasts: [ToastLevel, string, Record<string, string> | undefined][] = []
  const host = {
    sessionId: 's1',
    api: () => api,
    history: { history: new PromptHistory(['older']), push: (text: string) => void pushed.push(text) },
    cwd: () => '/work',
    isBusy: () => state.busy,
    hasDelayedTool: () => state.delayed,
    slashCommands: () => COMMANDS,
    runSlash: vi.fn<ComposerHost['runSlash']>(async () => {}),
    abort: vi.fn<ComposerHost['abort']>(async () => {}),
    toast: (level: ToastLevel, key: string, params?: Record<string, string>) =>
      void toasts.push([level, key, params]),
    reportError: vi.fn<ComposerHost['reportError']>(),
  } satisfies ComposerHost
  const composer = new SessionComposer(host)
  return { composer, host, api, state, pushed, toasts }
}

function type(composer: SessionComposer, text: string): void {
  composer.input(text, text.length)
}

describe('requestFocus', () => {
  it('counts the requests, which the box watches', () => {
    const { composer } = setup()
    composer.requestFocus()
    composer.requestFocus()
    expect(composer.focusRequests.value).toBe(2)
  })
})

describe('submit', () => {
  it('sends the text as a prompt and empties the box', async () => {
    const { composer, api, pushed } = setup()
    type(composer, 'hello')
    await composer.submit()
    expect(api.prompt).toHaveBeenCalledWith('s1', 'hello', [])
    expect(composer.draft.text).toBe('')
    expect(pushed).toEqual(['hello'])
  })

  it('sends nothing for a blank box', async () => {
    const { composer, api } = setup()
    type(composer, '  \n')
    await composer.submit()
    expect(api.prompt).not.toHaveBeenCalled()
  })

  it('puts the prompt back when sending fails', async () => {
    const { composer, api, host } = setup()
    const failure = new Error('not connected')
    api.prompt.mockRejectedValueOnce(failure)
    type(composer, 'precious')
    await composer.submit()
    expect(composer.draft.text).toBe('precious')
    expect(host.reportError).toHaveBeenCalledWith(failure)
  })

  it('runs a shell line and leaves shell mode', async () => {
    const { composer, api, pushed } = setup()
    composer.enterShell()
    type(composer, 'ls -la')
    await composer.submit()
    expect(api.shell).toHaveBeenCalledWith('s1', 'ls -la')
    expect(api.prompt).not.toHaveBeenCalled()
    expect(composer.draft.shell).toBe(false)
    expect(pushed).toEqual(['!ls -la'])
  })

  it('keeps a failed shell command in the box, still in shell mode', async () => {
    const { composer, api } = setup()
    api.shell.mockRejectedValueOnce(new Error('lost'))
    composer.enterShell()
    type(composer, 'make')
    await composer.submit()
    expect(composer.draft.shell).toBe(true)
    expect(composer.draft.text).toBe('make')
  })

  it('runs a built-in slash command with its arguments instead of sending it', async () => {
    const { composer, api, host, pushed } = setup()
    type(composer, '/rename Work')
    await composer.submit()
    expect(host.runSlash).toHaveBeenCalledWith('/rename Work')
    expect(api.prompt).not.toHaveBeenCalled()
    expect(pushed).toEqual(['/rename Work'])
    expect(composer.draft.text).toBe('')
  })

  it('expands a custom command and sends the result', async () => {
    const { composer, api } = setup()
    type(composer, '/review the diff')
    await composer.submit()
    expect(api.expandCommand).toHaveBeenCalledWith('review', 'the diff', '/work')
    expect(api.prompt).toHaveBeenCalledWith('s1', 'expanded text', [])
  })

  it('warns instead of sending when a custom command expands to nothing', async () => {
    const { composer, api, toasts } = setup()
    api.expandCommand.mockResolvedValueOnce(' ')
    await composer.runCustomCommand('review', '')
    expect(api.prompt).not.toHaveBeenCalled()
    expect(toasts).toEqual([['warn', 'composer.toast.expandedToNothing', undefined]])
  })

  it('sends an unknown slash line as a prompt', async () => {
    const { composer, api } = setup()
    type(composer, '/usr/bin/env is where')
    await composer.submit()
    expect(api.prompt).toHaveBeenCalledWith('s1', '/usr/bin/env is where', [])
  })

  it('blocks unknown single-token /foo typos with a warning toast and keeps the draft intact', async () => {
    const { composer, api, toasts } = setup()
    type(composer, '/udno')
    await composer.submit()
    expect(api.prompt).not.toHaveBeenCalled()
    expect(composer.draft.text).toBe('/udno')
    expect(toasts).toEqual([
      ['warn', 'composer.toast.unknownCommandSuggest', { name: 'udno', suggestion: 'undo' }],
    ])

    toasts.splice(0)
    type(composer, '/notacommandatall')
    await composer.submit()
    expect(api.prompt).not.toHaveBeenCalled()
    expect(composer.draft.text).toBe('/notacommandatall')
    expect(toasts).toEqual([['warn', 'composer.toast.unknownCommand', { name: 'notacommandatall' }]])
  })

  it('sends the attached images with the text', async () => {
    const { composer, api } = setup()
    composer.draft.text = 'look [Image 1] '
    composer.draft.attachments = [
      { token: '[Image 1]', image: { mime: 'image/png', data: 'AAA' }, name: undefined },
    ]
    await composer.submit()
    expect(api.prompt).toHaveBeenCalledWith('s1', 'look [Image 1] ', [{ mime: 'image/png', data: 'AAA' }])
  })
})

describe('history', () => {
  it('walks back to the older prompt and returns the draft', () => {
    const { composer } = setup()
    type(composer, 'draft')
    expect(composer.historyPrev()).toBe('older'.length)
    expect(composer.draft.text).toBe('older')
    composer.historyNext()
    expect(composer.draft.text).toBe('draft')
  })

  it('enters shell mode for a `!` entry', () => {
    const { composer, host } = setup()
    host.history.history.push('!ls')
    composer.historyPrev()
    expect(composer.draft.shell).toBe(true)
    expect(composer.draft.text).toBe('ls')
  })

  it('starts over from the newest entry after typing', () => {
    const { composer, host } = setup()
    host.history.history.push('newer')
    composer.historyPrev()
    composer.historyPrev()
    type(composer, 'x')
    composer.historyPrev()
    expect(composer.draft.text).toBe('newer')
  })

  it('filters history to !-prefixed entries only while browsing in shell mode', () => {
    const { composer, host } = setup()
    host.history.history.push('!git status')
    host.history.history.push('normal prompt')
    host.history.history.push('!cargo check')
    composer.enterShell()
    type(composer, 'ls')

    expect(composer.historyPrev()).toBe('cargo check'.length)
    expect(composer.draft.shell).toBe(true)
    expect(composer.draft.text).toBe('cargo check')

    expect(composer.historyPrev()).toBe('git status'.length)
    expect(composer.draft.text).toBe('git status')
    expect(composer.historyPrev()).toBeUndefined()

    composer.historyNext()
    composer.historyNext()
    expect(composer.draft.shell).toBe(true)
    expect(composer.draft.text).toBe('ls')
  })
})

describe('completion', () => {
  it('inserts the chosen command and runs one without arguments on Enter', () => {
    const { composer, host, pushed } = setup()
    type(composer, '/re')
    expect(composer.completion?.items.map((i) => i.label)).toEqual(['/rename', '/review'])
    composer.acceptCompletion(false)
    expect(composer.draft.text).toBe('/rename ')

    type(composer, '/und')
    composer.acceptCompletion(true)
    expect(host.runSlash).toHaveBeenCalledWith('/undo')
    expect(pushed).toEqual(['/undo'])
    expect(composer.draft.text).toBe('')
  })

  it('runs a fully typed command that takes arguments, without them', async () => {
    const { composer, host } = setup()
    type(composer, '/rename')
    composer.acceptCompletion(true)
    await Promise.resolve()
    expect(host.runSlash).toHaveBeenCalledWith('/rename')
  })

  it('completes standalone-capable commands without trailing space on Tab and runs partial ones on Enter', async () => {
    const { composer, host, api, pushed } = setup()
    type(composer, '/ses')
    const caret = composer.acceptCompletion(false)
    expect(composer.draft.text).toBe('/sessions')
    expect(caret).toBe('/sessions'.length)
    expect(composer.completion).toBeUndefined()
    expect(host.runSlash).not.toHaveBeenCalled()

    type(composer, '/ses')
    expect(composer.acceptCompletion(true)).toBeUndefined()
    expect(host.runSlash).toHaveBeenCalledWith('/sessions')
    expect(pushed).toEqual(['/sessions'])
    expect(composer.draft.text).toBe('')

    // Custom commands with optional args also execute standalone on Enter from a partial token.
    type(composer, '/rev')
    expect(composer.acceptCompletion(true)).toBeUndefined()
    await Promise.resolve()
    expect(api.expandCommand).toHaveBeenCalledWith('review', '', '/work')

    // Partial required-arg command completes with space on Enter instead of executing.
    type(composer, '/ren')
    expect(composer.acceptCompletion(true)).toBe('/rename '.length)
    expect(composer.draft.text).toBe('/rename ')
    expect(host.runSlash).toHaveBeenCalledTimes(1)
  })

  it('replaces only the mid-text slash token without submitting or stripping surrounding text', () => {
    const { composer, host, api } = setup()
    composer.input('please run /un in chat', 14)
    const tabCaret = composer.acceptCompletion(false)
    expect(composer.draft.text).toBe('please run /undo in chat')
    expect(tabCaret).toBe('please run /undo'.length)
    expect(composer.completion).toBeUndefined()
    expect(host.runSlash).not.toHaveBeenCalled()
    expect(api.prompt).not.toHaveBeenCalled()

    composer.input('please run /un', 14)
    const enterCaret = composer.acceptCompletion(true)
    expect(composer.draft.text).toBe('please run /undo')
    expect(enterCaret).toBe('please run /undo'.length)
    expect(composer.completion).toBeUndefined()
    expect(host.runSlash).not.toHaveBeenCalled()
    expect(api.prompt).not.toHaveBeenCalled()
    composer.input('please /ren Work', 11)
    const midSpaceCaret = composer.acceptCompletion(true)
    expect(composer.draft.text).toBe('please /rename Work')
    expect(midSpaceCaret).toBe('please /rename '.length)

    composer.input('/ren Work', 4)
    const leadSpaceCaret = composer.acceptCompletion(false)
    expect(composer.draft.text).toBe('/rename Work')
    expect(leadSpaceCaret).toBe('/rename '.length)
  })
  it('replaces the mention with the chosen file', async () => {
    vi.useFakeTimers()
    const { composer, api } = setup()
    api.listFiles.mockResolvedValue(['src/main.ts'])
    type(composer, 'see @sr and more')
    composer.input('see @sr and more', 7)
    await vi.advanceTimersByTimeAsync(100)
    expect(api.listFiles).toHaveBeenCalledWith('/work', 'sr', 20)
    const caret = composer.acceptCompletion(false)
    expect(composer.draft.text).toBe('see @src/main.ts  and more')
    expect(caret).toBe('see @src/main.ts '.length)
    vi.useRealTimers()
  })
  it('completes shell args via completeShell and triggers 0ms shell completion on Tab', async () => {
    vi.useFakeTimers()
    const { composer, api } = setup()
    api.completeShell.mockImplementation(async (_cwd, line) => {
      if (line === 'docker ') return [{ value: 'compose', description: 'Multi-container' }]
      if (line === 'docker --f') return [{ value: '--format=', description: 'Output format' }]
      return []
    })

    composer.enterShell()
    type(composer, 'docker ')
    composer.triggerShellCompletion()
    await vi.advanceTimersByTimeAsync(0)
    expect(api.completeShell).toHaveBeenCalledWith('/work', 'docker ', { cursor: 7, limit: 50 })
    const caret = composer.acceptCompletion(false)
    expect(composer.draft.text).toBe('docker compose ')
    expect(caret).toBe('docker compose '.length)

    type(composer, 'docker --f')
    composer.triggerShellCompletion()
    await vi.advanceTimersByTimeAsync(0)
    composer.acceptCompletion(false)
    expect(composer.draft.text).toBe('docker --format=')
    vi.useRealTimers()
  })
})

describe('Esc', () => {
  beforeEach(() => vi.useFakeTimers())

  it('closes the list first, then leaves shell mode', () => {
    const { composer, host } = setup({ busy: true })
    type(composer, '/u')
    composer.escape()
    expect(composer.completion).toBeUndefined()
    composer.enterShell()
    composer.escape()
    expect(composer.draft.shell).toBe(false)
    expect(host.abort).not.toHaveBeenCalled()
  })

  it('interrupts a busy session on the second press only', () => {
    const { composer, host } = setup({ busy: true })
    composer.escape()
    expect(composer.interruptArmed.value).toBe(true)
    expect(host.abort).not.toHaveBeenCalled()
    composer.escape()
    expect(host.abort).toHaveBeenCalledTimes(1)
    expect(composer.interruptArmed.value).toBe(false)
  })

  it('needs the second press within the window', () => {
    const { composer, host } = setup({ busy: true })
    composer.escape()
    vi.advanceTimersByTime(INTERRUPT_ARMED_MS)
    expect(composer.interruptArmed.value).toBe(false)
    composer.escape()
    expect(host.abort).not.toHaveBeenCalled()
  })

  it('does nothing for an idle session', () => {
    const { composer, host } = setup()
    composer.escape()
    expect(composer.interruptArmed.value).toBe(false)
    expect(host.abort).not.toHaveBeenCalled()
  })

  it('cancels a delayed tool call with one press', () => {
    const { composer, host, toasts } = setup({ busy: true, delayed: true })
    composer.escape()
    expect(host.abort).toHaveBeenCalledTimes(1)
    expect(toasts).toEqual([['info', 'composer.toast.delayedCancelled', undefined]])
  })
})

describe('taking prompts back', () => {
  const image: NamedImage = { mime: 'image/png', data: 'AAA', name: 'a.png' }

  it('puts a withdrawn queued prompt back into the box', async () => {
    const { composer, api } = setup()
    api.removeQueued.mockResolvedValueOnce({ removed: true, text: 'wait [Image 1]', images: [image] })
    type(composer, 'typing')
    await composer.removeQueued('m9')
    expect(api.removeQueued).toHaveBeenCalledWith('s1', 'm9')
    expect(composer.draft.text).toBe('typing\n\nwait [Image 1]')
    expect(composer.draft.attachments).toHaveLength(1)
    expect(composer.removingQueued.has('m9')).toBe(false)
  })

  it('says so when the prompt was delivered meanwhile', async () => {
    const { composer, api, toasts } = setup()
    api.removeQueued.mockResolvedValueOnce({ removed: false, text: '', images: [] })
    await composer.removeQueued('m9')
    expect(composer.draft.text).toBe('')
    expect(toasts).toEqual([['info', 'composer.toast.alreadySent', undefined]])
  })

  it('takes a restored draft from an undo of its own session only', () => {
    const { composer } = setup()
    emitDraftRestored('other', 'not mine', [])
    expect(composer.draft.text).toBe('')
    emitDraftRestored('s1', 'mine', [])
    expect(composer.draft.text).toBe('mine')
    composer.dispose()
    emitDraftRestored('s1', 'late', [])
    expect(composer.draft.text).toBe('mine')
  })
})

describe('clear', () => {
  it('keeps a long text in the history, not a short one', () => {
    const { composer, pushed } = setup()
    type(composer, 'short')
    composer.clear()
    type(composer, 'a text that is clearly longer than twenty characters')
    composer.clear()
    expect(pushed).toEqual(['a text that is clearly longer than twenty characters'])
    expect(composer.draft.text).toBe('')
  })
})

describe('images and dropped files', () => {
  it('attaches supported images and inserts absolute paths for unsupported files from text/uri-list', async () => {
    const { composer, toasts } = setup()
    type(composer, 'see ')
    const png = new File([new Uint8Array([1, 2, 3])], 'shot.png', { type: 'image/png' })
    const text = new File(['x'], 'notes.txt', { type: 'text/plain' })
    await composer.attachFiles([png, text], 'file:///work/shot.png\nfile:///work/notes.txt')
    expect(composer.draft.text).toBe('see [Image 1] /work/notes.txt ')
    expect(composer.draft.attachments[0]!.image).toEqual({ mime: 'image/png', data: 'AQID' })
    expect(toasts).toEqual([])
  })

  it('falls back to the filename when the browser withholds the path', async () => {
    const { composer, toasts } = setup()
    type(composer, 'see')
    const text = new File(['x'], 'notes.txt', { type: 'text/plain' })
    await composer.attachFiles([text])
    expect(composer.draft.text).toBe('see notes.txt ')
    expect(toasts).toEqual([])
  })

  it('inserts paths from pasted file:// URIs without File objects', async () => {
    const { composer, toasts } = setup()
    await composer.attachFiles([], 'file:///home/me/my%20file.rs\r\n# comment\r\nfile:///home/me/lib.ts')
    expect(composer.draft.text).toBe('/home/me/my file.rs /home/me/lib.ts ')
    expect(toasts).toEqual([])
  })
})
