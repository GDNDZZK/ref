import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CompletionController, FILE_QUERY_DELAY_MS, parseShellContext, slashItems } from './completion.ts'
import type { CompletionDeps, SlashCommand } from './completion.ts'

const COMMANDS: SlashCommand[] = [
  { name: 'undo', aliases: [], title: 'Undo the last prompt', requiresArgs: false, custom: false },
  { name: 'new', aliases: ['clear'], title: 'New session', requiresArgs: false, custom: false },
  { name: 'review', aliases: [], title: 'Review the diff', requiresArgs: false, custom: true },
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
      { value: '/new', label: '/clear → /new', description: 'New session' },
    ])
  })

  it('appends a trailing space only when a command requires arguments', () => {
    const commands: SlashCommand[] = [
      { name: 'rename', aliases: [], title: 'Rename', requiresArgs: true, custom: false },
      { name: 'sessions', aliases: [], title: 'Sessions', requiresArgs: false, custom: false },
      { name: 'review', aliases: [], title: 'Review', requiresArgs: false, custom: true },
    ]
    expect(slashItems(commands, '').map((item) => item.value)).toEqual(['/rename ', '/sessions', '/review'])
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
    expect(controller.selectedItem?.value).toBe('/undo')
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

  it('triggers mid-text slash completion and skips path-like slashes', async () => {
    const listDirs = vi.fn(async (path: string) => ({ path, dirs: ['src', 'tmp'], files: [] }))
    const controller = new CompletionController({
      commands: () => [
        ...COMMANDS,
        { name: 'cd', aliases: [], title: 'cd', requiresArgs: false, custom: false },
      ],
      listFiles: async () => [],
      listDirs,
      reportError: () => {},
    })

    controller.update('please run /un', 14, false, true)
    expect(controller.completion).toMatchObject({ kind: 'slash', start: 11, end: 14 })
    expect(controller.selectedItem?.value).toBe('/undo')

    controller.update('hello /un world', 8, false, true)
    expect(controller.completion).toMatchObject({ kind: 'slash', start: 6, end: 9 })
    expect(controller.selectedItem?.value).toBe('/undo')

    controller.update('line 1\n/re', 10, false, true)
    expect(controller.completion).toMatchObject({ kind: 'slash', start: 7, end: 10 })
    expect(controller.selectedItem?.value).toBe('/review')

    controller.update('check src/main.rs', 17, false, true)
    expect(controller.completion).toBeUndefined()

    controller.update('visit https://example.com', 25, false, true)
    expect(controller.completion).toBeUndefined()

    controller.update('check /usr/bin', 14, false, true)
    expect(controller.completion).toBeUndefined()

    controller.update('check /usr/bin', 8, false, true)
    expect(controller.completion).toBeUndefined()

    controller.update('/usr/bin', 2, false, true)
    expect(controller.completion).toBeUndefined()

    controller.update('// comment', 1, false, true)
    expect(controller.completion).toBeUndefined()

    controller.update('/', 0, false, true)
    expect(controller.completion).toBeUndefined()

    controller.update('check /foo\\bar', 8, false, true)
    expect(controller.completion).toBeUndefined()
    controller.update('/cd src', 7, false, true)
    await settle()
    expect(controller.completion?.kind).toBe('path')
    expect(listDirs).toHaveBeenCalledWith('.', { cwd: undefined, includeFiles: false })

    controller.update('/cd /tmp', 8, false, true)
    await settle()
    expect(controller.completion?.kind).toBe('path')
    expect(listDirs).toHaveBeenCalledWith('/', { cwd: undefined, includeFiles: false })
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
        { name: 'cd', aliases: ['cwd'], title: 'cd', requiresArgs: false, custom: false },
        { name: 'export', aliases: [], title: 'export', requiresArgs: false, custom: false },
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
        { name: 'models', aliases: ['model'], title: 'Models', requiresArgs: false, custom: false },
        { name: 'agents', aliases: ['agent'], title: 'Agents', requiresArgs: false, custom: false },
        { name: 'variant', aliases: [], title: 'Variant', requiresArgs: false, custom: false },
        { name: 'sessions', aliases: [], title: 'Sessions', requiresArgs: false, custom: false },
        { name: 'close-session', aliases: [], title: 'Close', requiresArgs: false, custom: false },
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
    expect(parseShellContext('ls -la', 6)).toMatchObject({
      role: 'arg',
      segmentIndex: 0,
      tokenStart: 3,
      token: '-la',
      isPath: false,
      atLineStart: false,
    })
    expect(parseShellContext('docker ', 7)).toMatchObject({
      role: 'arg',
      segmentIndex: 0,
      tokenStart: 7,
      token: '',
      isPath: false,
      atLineStart: false,
    })
    expect(parseShellContext('cat C:\\foo', 10)).toMatchObject({
      role: 'arg',
      token: 'C:\\foo',
      isPath: true,
    })
    expect(parseShellContext('cat D:bar', 9)).toMatchObject({ role: 'arg', token: 'D:bar', isPath: true })
    expect(parseShellContext('ls -la ./sr', 11)).toMatchObject({ role: 'arg', token: './sr', isPath: true })

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

  it('skips shell wrappers, wrapper flags, durations, and env assignments with accurate atLineStart', async () => {
    expect(parseShellContext('car', 3)).toMatchObject({ role: 'command', token: 'car', atLineStart: true })
    expect(parseShellContext('FOO=1 car', 9)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('sudo ap', 7)).toMatchObject({
      role: 'command',
      token: 'ap',
      atLineStart: false,
    })
    expect(parseShellContext('sudo -u root ap', 15)).toMatchObject({
      role: 'command',
      token: 'ap',
      atLineStart: false,
    })
    expect(parseShellContext('nice -n 10 car', 14)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('timeout 5s car', 14)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('timeout -k 2s 5s car', 20)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('taskset 0x1 car', 15)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('taskset -c 0-3 car', 18)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('env -i FOO=1 sudo -E BAR=2 car', 30)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('sudo -- car', 11)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('/usr/bin/env py', 15)).toMatchObject({
      role: 'command',
      token: 'py',
      atLineStart: false,
    })
    expect(parseShellContext('/usr/bin/sudo car', 17)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('/usr/bin/sudo -u root car', 25)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('sudo -Euroot car', 16)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('sudo -Eu root car', 17)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })
    expect(parseShellContext('timeout -vk5s 5s car', 20)).toMatchObject({
      role: 'command',
      token: 'car',
      atLineStart: false,
    })

    const wrappers = [
      'sudo',
      'doas',
      'env',
      'nohup',
      'time',
      'nice',
      'ionice',
      'setsid',
      'strace',
      'ltrace',
      'stdbuf',
      'watch',
      'command',
      'exec',
      'builtin',
      'xargs',
      'caffeinate',
      'pkexec',
    ]
    for (const wrapper of wrappers) {
      expect(parseShellContext(`${wrapper} ap`, wrapper.length + 3)).toMatchObject({
        role: 'command',
        token: 'ap',
        atLineStart: false,
      })
    }
    expect(parseShellContext('chrt 50 ap', 10)).toMatchObject({
      role: 'command',
      token: 'ap',
      atLineStart: false,
    })

    // Wrapper flags, flag arguments, and wrapper positional args stay closed.
    expect(parseShellContext('sudo -', 6)).toBeUndefined()
    expect(parseShellContext('sudo -u', 7)).toBeUndefined()
    expect(parseShellContext('sudo -u ', 8)).toBeUndefined()
    expect(parseShellContext('sudo -u ro', 10)).toBeUndefined()
    expect(parseShellContext('timeout ', 8)).toBeUndefined()
    expect(parseShellContext('timeout 5', 9)).toBeUndefined()

    // Arguments after the wrapped command resolve as shell args.
    expect(parseShellContext('sudo apt install ./foo', 22)).toMatchObject({
      role: 'arg',
      token: './foo',
      isPath: true,
      atLineStart: false,
    })

    const listBinaries = vi.fn(async () => ['apt', 'apt-get'])
    const listDirs = vi.fn(async () => ({ path: '/repo/src', dirs: [], files: ['foo.ts'] }))
    const controller = new CompletionController({
      cwd: () => '/repo',
      commands: () => COMMANDS,
      listFiles: async () => [],
      listBinaries,
      listDirs,
      shellHistory: () => ['!apt update', '!sudo apt upgrade'],
      reportError: () => {},
    })

    // `!sudo ap` completes binaries without matching full-line `!` history from offset 0.
    controller.update('sudo ap', 7, true, true)
    expect(controller.completion).toBeUndefined()
    await settle()
    expect(listBinaries).toHaveBeenCalledWith('ap', 50)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['apt ', 'apt-get '])
    expect(controller.completion?.start).toBe(5)

    controller.update('sudo apt install ./src/', 23, true, true)
    await settle()
    expect(controller.completion?.kind).toBe('path')
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['./src/foo.ts '])
  })
  it('completes shell subcommands and flags via completeShell with trailing space rules, fallback, and 0ms Tab triggers', async () => {
    const completeShell = vi.fn(async (_cwd: string, line: string) => {
      if (line === 'docker ') {
        return [
          { value: 'compose', description: 'Define and run multi-container apps' },
          { value: 'build', description: 'Build an image' },
        ]
      }
      if (line === 'docker comp') {
        return [
          { value: 'compose', description: 'Compose plugin' },
          { value: '--format=', description: 'Format output' },
          { value: 'dir/' },
          { value: 'win\\' },
          { value: 'host:' },
        ]
      }
      if (line === 'docker --') {
        return [{ value: '--debug', description: 'Enable debug mode' }]
      }
      return []
    })
    const listBinaries = vi.fn(async () => ['cargo', 'docker'])
    const listDirs = vi.fn(async (path: string) => ({ path, dirs: ['src'], files: ['README.md'] }))
    const controller = new CompletionController({
      cwd: () => '/repo',
      commands: () => COMMANDS,
      listFiles: async () => [],
      listBinaries,
      listDirs,
      completeShell,
      shellHistory: () => ['!docker ps'],
      reportError: () => {},
    })

    // Typing `docker comp` debounces completeShell and applies trailing-space rules + icon + description.
    controller.update('docker comp', 11, true, true)
    expect(completeShell).not.toHaveBeenCalled()
    await settle()
    expect(completeShell).toHaveBeenCalledWith('/repo', 'docker comp', { cursor: 11, limit: 50 })
    expect(controller.completion).toMatchObject({
      kind: 'shell',
      freeformEnter: true,
      start: 7,
      end: 11,
    })
    expect(controller.completion?.items).toEqual([
      { value: 'compose ', label: 'compose', description: 'Compose plugin', icon: 'terminal-16' },
      { value: '--format=', label: '--format=', description: 'Format output', icon: 'terminal-16' },
      { value: 'dir/', label: 'dir/', description: '', icon: 'terminal-16' },
      { value: 'win\\', label: 'win\\', description: '', icon: 'terminal-16' },
      { value: 'host:', label: 'host:', description: '', icon: 'terminal-16' },
    ])

    // `-`-prefixed tokens call completeShell; non-empty shows flags, empty closes without falling back to listDirs.
    controller.update('docker --', 9, true, true)
    await settle()
    expect(controller.completion?.items).toEqual([
      { value: '--debug ', label: '--debug', description: 'Enable debug mode', icon: 'terminal-16' },
    ])
    controller.update('docker --unknown', 16, true, true)
    await settle()
    expect(controller.completion).toBeUndefined()
    expect(listDirs).not.toHaveBeenCalled()

    // Non-`-` arg returning [] falls back to listDirs with files + trailing space.
    controller.update('cat R', 5, true, true)
    await settle()
    expect(completeShell).toHaveBeenCalledWith('/repo', 'cat R', { cursor: 5, limit: 50 })
    expect(listDirs).toHaveBeenCalledWith('.', { cwd: '/repo', includeFiles: true })
    expect(controller.completion?.kind).toBe('path')
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['README.md '])

    // Explicit Tab triggers run at 0ms (empty `!`, after `| `, and after `docker `).
    listBinaries.mockClear()
    completeShell.mockClear()
    controller.triggerShell('', 0)
    await vi.advanceTimersByTimeAsync(0)
    expect(listBinaries).toHaveBeenCalledWith('', 50)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['docker ps', 'cargo ', 'docker '])

    controller.triggerShell('ls | ', 5)
    await vi.advanceTimersByTimeAsync(0)
    expect(listBinaries).toHaveBeenLastCalledWith('', 50)
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['cargo ', 'docker '])

    controller.triggerShell('docker ', 7)
    await vi.advanceTimersByTimeAsync(0)
    expect(completeShell).toHaveBeenCalledWith('/repo', 'docker ', { cursor: 7, limit: 50 })
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['compose ', 'build '])
  })

  it('ignores stale completeShell responses when a newer query is active', async () => {
    let resolveFirst: (items: { value: string; description?: string }[]) => void = () => {}
    const completeShell = vi.fn((_cwd: string, line: string) =>
      line === 'docker c'
        ? new Promise<{ value: string; description?: string }[]>((r) => (resolveFirst = r))
        : Promise.resolve([{ value: 'compose', description: 'Compose' }]),
    )
    const controller = new CompletionController({
      cwd: () => '/repo',
      commands: () => COMMANDS,
      listFiles: async () => [],
      completeShell,
      reportError: () => {},
    })

    controller.update('docker c', 8, true, true)
    await settle()
    controller.update('docker co', 9, true, true)
    resolveFirst([{ value: 'config', description: 'Stale' }])
    await vi.advanceTimersByTimeAsync(0)
    expect(controller.completion).toBeUndefined()

    await settle()
    expect(controller.completion?.items.map((i) => i.value)).toEqual(['compose '])
  })

  it('passes UTF-8 byte cursor offset to completeShell for non-ASCII input while preserving UTF-16 token range', async () => {
    const completeShell = vi.fn(async () => [{ value: 'checkout', description: 'Switch branches' }])
    const controller = new CompletionController({
      cwd: () => '/repo',
      commands: () => COMMANDS,
      listFiles: async () => [],
      completeShell,
      reportError: () => {},
    })

    const line = 'echo 你好 && git ch'
    controller.update(line, line.length, true, true)
    await settle()
    expect(completeShell).toHaveBeenCalledWith('/repo', line, {
      cursor: new TextEncoder().encode(line).length,
      limit: 50,
    })
    expect(controller.completion).toMatchObject({ start: 15, end: 17 })
    expect(controller.selectedItem?.value).toBe('checkout ')

    const midPrefix = 'git commit -m "中" --'
    const midLine = `${midPrefix} --help`
    controller.update(midLine, midPrefix.length, true, true)
    await settle()
    expect(completeShell).toHaveBeenLastCalledWith('/repo', midLine, {
      cursor: 22,
      limit: 50,
    })
    expect(controller.completion).toMatchObject({ start: 18, end: 20 })
  })
})
