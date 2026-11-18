import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CompletionController, FILE_QUERY_DELAY_MS, parseShellContext, slashItems } from './completion.ts'
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

  it('keeps the selection when a non-empty slash query is re-checked without editing, and resets when the query changes', () => {
    const { controller } = setup()
    controller.update('/e', 2, false, true)
    expect(controller.completion?.items.map((item) => item.label)).toEqual(['/new', '/review'])
    controller.move(1)
    expect(controller.selectedItem?.label).toBe('/review')
    controller.update('/e', 2, false, false)
    expect(controller.selectedItem?.label).toBe('/review')
    controller.update('/ew', 3, false, true)
    expect(controller.selectedItem?.label).toBe('/new')
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

  it('completes /cd and /export paths with session cwd and 0-flicker parent-dir cache', async () => {
    const listDirs = vi.fn(async (path: string) => ({
      path,
      dirs: ['crates', 'docs', '.git'],
      files: ['README.md'],
    }))
    const controller = new CompletionController({
      sessionId: 'ses_7',
      cwd: () => '/repo',
      commands: () => [
        ...COMMANDS,
        { name: 'cd', aliases: ['cwd'], title: 'cd', takesArgs: true, custom: false },
        { name: 'export', aliases: [], title: 'export', takesArgs: true, custom: false },
      ],
      listFiles: async () => [],
      listDirs,
      reportError: () => {},
    })

    controller.update('/cd ', 4, false, true)
    await settle()
    expect(listDirs).toHaveBeenCalledWith('.', { cwd: '/repo', includeFiles: false })
    expect(controller.completion).toMatchObject({
      kind: 'path',
      explicitlySelected: false,
      freeformEnter: true,
    })
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['crates/', 'docs/'])

    // Typing additional letters in the same parent directory resolves synchronously from cache (0 extra RPCs).
    controller.update('/cd cr', 6, false, true)
    expect(listDirs).toHaveBeenCalledTimes(1)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['crates/'])

    // /export includes default ref-session-<id>.md and regular files.
    controller.update('/export ', 8, false, true)
    await settle()
    expect(controller.completion?.items.map((i) => i.value)).toEqual([
      'ref-session-ses_7.md',
      'crates/',
      'docs/',
      'README.md',
    ])
  })

  it('completes /models, /agents, /variant, /sessions, multi-token /close-session, and @file in custom commands', async () => {
    const listFiles = vi.fn(async () => ['src/index.ts'])
    const controller = new CompletionController({
      commands: () => [
        ...COMMANDS,
        { name: 'models', aliases: ['model'], title: 'Models', takesArgs: true, custom: false },
        { name: 'agents', aliases: ['agent'], title: 'Agents', takesArgs: true, custom: false },
        { name: 'variant', aliases: [], title: 'Variant', takesArgs: true, custom: false },
        { name: 'sessions', aliases: [], title: 'Sessions', takesArgs: true, custom: false },
        { name: 'close-session', aliases: [], title: 'Close', takesArgs: true, custom: false },
      ],
      listFiles,
      models: () => [
        { provider: 'anthropic', providerName: 'Anthropic', id: 'claude-sonnet-4-5', name: 'Sonnet' },
      ],
      agents: () => [{ name: 'build', description: 'Builder' }],
      variants: () => ['low', 'high'],
      sessions: () => [{ id: 's1', name: 'alpha', title: 'Alpha', cwd: '/a', live: true }],
      liveSessions: () => [
        { id: 's1', name: 'alpha', title: 'Alpha', cwd: '/a', live: true },
        { id: 's2', name: 'beta', title: 'Beta', cwd: '/b', live: true },
      ],
      reportError: () => {},
    })

    controller.update('/model son', 10, false, true)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['anthropic/claude-sonnet-4-5'])

    controller.update('/agent bu', 9, false, true)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['build'])

    controller.update('/variant ', 9, false, true)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['default', 'low', 'high'])

    controller.update('/sessions al', 12, false, true)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['alpha'])

    // Multi-token /close-session excludes already-listed sessions.
    controller.update('/close-session alpha ', 21, false, true)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['beta '])
    expect(controller.completion?.freeformEnter).toBe(true)

    // Custom command `/review` supports @file mentions inside arguments.
    controller.update('/review check @src/', 19, false, true)
    await settle()
    expect(listFiles).toHaveBeenCalledWith('src/')
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['@src/index.ts '])
  })

  it('completes ! shell history, binaries after pipes/&&, and non-flag path arguments', async () => {
    expect(parseShellContext('git status && car', 17)).toMatchObject({
      role: 'command',
      segmentIndex: 1,
      token: 'car',
    })
    expect(parseShellContext('ls -la', 6)).toBeUndefined()
    expect(parseShellContext('ls -la ./sr', 11)).toMatchObject({ role: 'arg', token: './sr' })

    const listBinaries = vi.fn(async () => ['cargo', 'cargo-clippy'])
    const listDirs = vi.fn(async () => ({ path: '/repo/src', dirs: ['stores'], files: ['main.ts'] }))
    const controller = new CompletionController({
      cwd: () => '/repo',
      commands: () => COMMANDS,
      listFiles: async () => [],
      listBinaries,
      listDirs,
      shellHistory: () => ['hello', '!cargo test --workspace'],
      reportError: () => {},
    })

    // Empty shell input stays closed so Esc/Backspace exit shell mode cleanly.
    controller.update('', 0, true, true)
    expect(controller.completion).toBeUndefined()

    controller.update('car', 3, true, true)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['cargo test --workspace'])
    await settle()
    expect(listBinaries).toHaveBeenCalledWith('car', 50)
    expect(controller.completion?.items.map((i) => i.value)).toEqual([
      'cargo test --workspace',
      'cargo ',
      'cargo-clippy ',
    ])
    expect(controller.completion?.explicitlySelected).toBe(false)
    controller.move(1)
    expect(controller.completion?.explicitlySelected).toBe(true)

    // Argument path preserves ./ prefix.
    controller.update('ls ./src/', 9, true, true)
    await settle()
    expect(listDirs).toHaveBeenCalledWith('./src/', { cwd: '/repo', includeFiles: true })
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['./src/stores/', './src/main.ts '])
  })
})
