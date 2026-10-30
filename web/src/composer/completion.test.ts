import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CompletionController, FILE_QUERY_DELAY_MS, slashItems } from './completion.ts'
import type { CompletionDeps, SlashCommand } from './completion.ts'

const COMMANDS: SlashCommand[] = [
  { name: 'undo', aliases: [], title: 'Undo the last prompt', takesArgs: false, custom: false },
  { name: 'new', aliases: ['clear'], title: 'New session', takesArgs: false, custom: false },
  { name: 'review', aliases: [], title: 'Review the diff', takesArgs: true, custom: true },
]

describe('slashItems', () => {
  it('lists every command for an empty query', () => {
    expect(slashItems(COMMANDS, '').map((item) => item.label)).toEqual(['/undo', '/new', '/review'])
  })

  it('matches on the name and ranks built-in commands above custom ones', () => {
    const commands = [...COMMANDS, { ...COMMANDS[0]!, name: 'unbox', custom: true }]
    expect(slashItems(commands, 'un').map((item) => item.label)).toEqual(['/undo', '/unbox'])
  })

  it('shows an alias that matched as pointing at its command', () => {
    expect(slashItems(COMMANDS, 'cle')).toEqual([
      { value: '/new ', label: '/clear → /new', description: 'New session' },
    ])
  })

  it('finds nothing for text that matches nothing', () => {
    expect(slashItems(COMMANDS, 'zzz')).toEqual([])
  })
})

describe('CompletionController', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  function setup(files: (query: string) => Promise<string[]> = async () => ['src/main.ts', 'src/mod.ts']) {
    const listFiles = vi.fn(files)
    const reportError = vi.fn()
    const deps: CompletionDeps = { commands: () => COMMANDS, listFiles, reportError }
    return { controller: new CompletionController(deps), listFiles, reportError }
  }

  async function settle(): Promise<void> {
    await vi.advanceTimersByTimeAsync(FILE_QUERY_DELAY_MS)
  }

  it('opens the command list while a command is typed', () => {
    const { controller } = setup()
    controller.update('/u', 2, false, true)
    expect(controller.completion?.kind).toBe('slash')
    expect(controller.selectedItem?.value).toBe('/undo ')
  })

  it('closes once the command has arguments or the text is not a command', () => {
    const { controller } = setup()
    controller.update('/', 1, false, true)
    expect(controller.completion?.items.map((item) => item.label)).toEqual(['/undo', '/new', '/review'])
    controller.update('/undo now', 9, false, true)
    expect(controller.completion).toBeUndefined()
    controller.update('hello', 5, false, true)
    expect(controller.completion).toBeUndefined()
  })

  it('stays closed in shell mode', () => {
    const { controller } = setup()
    controller.update('/u', 2, true, true)
    expect(controller.completion).toBeUndefined()
  })

  it('moves the selection round', () => {
    const { controller } = setup()
    controller.update('/', 1, false, true)
    controller.move(-1)
    expect(controller.selectedItem?.label).toBe('/review')
    controller.move(1)
    expect(controller.selectedItem?.label).toBe('/undo')
  })

  it('stays closed after Esc until the text changes', () => {
    const { controller } = setup()
    controller.update('/u', 2, false, true)
    controller.dismiss()
    expect(controller.completion).toBeUndefined()
    controller.update('/u', 2, false, false)
    expect(controller.completion).toBeUndefined()
    controller.update('/un', 3, false, true)
    expect(controller.completion?.items.map((item) => item.label)).toEqual(['/undo'])
  })

  it('lists files for a mention after a short delay', async () => {
    const { controller, listFiles } = setup()
    controller.update('see @src/m', 10, false, true)
    expect(listFiles).not.toHaveBeenCalled()
    await settle()
    expect(listFiles).toHaveBeenCalledWith('src/m')
    expect(controller.completion).toMatchObject({ kind: 'file', start: 4 })
    expect(controller.selectedItem?.value).toBe('@src/main.ts ')
  })

  it('asks once for a burst of keys', async () => {
    const { controller, listFiles } = setup()
    controller.update('@s', 2, false, true)
    await vi.advanceTimersByTimeAsync(FILE_QUERY_DELAY_MS - 1)
    controller.update('@sr', 3, false, true)
    await settle()
    expect(listFiles.mock.calls).toEqual([['sr']])
  })

  it('ignores an answer for a query that is no longer typed', async () => {
    let answer: (files: string[]) => void = () => {}
    const { controller, listFiles } = setup(() => new Promise((resolve) => (answer = resolve)))
    controller.update('@a', 2, false, true)
    await settle()
    controller.update('@ab', 3, false, true)
    answer(['a.ts'])
    await vi.advanceTimersByTimeAsync(0)
    expect(controller.completion).toBeUndefined()

    // The newer query waits for the running request, then goes out.
    await settle()
    expect(listFiles.mock.calls).toEqual([['a'], ['ab']])
  })

  it('closes when no file matches', async () => {
    const { controller } = setup(async () => [])
    controller.update('@zzz', 4, false, true)
    await settle()
    expect(controller.completion).toBeUndefined()
  })

  it('reports a failed request', async () => {
    const failure = new Error('no connection')
    const { controller, reportError } = setup(async () => {
      throw failure
    })
    controller.update('@a', 2, false, true)
    await settle()
    expect(reportError).toHaveBeenCalledWith(failure)
  })

  it('swaps the command list for the file list', async () => {
    const { controller } = setup()
    controller.update('/', 1, false, true)
    controller.update('@', 1, false, true)
    expect(controller.completion).toBeUndefined()
    await settle()
    expect(controller.completion?.kind).toBe('file')
  })
})
