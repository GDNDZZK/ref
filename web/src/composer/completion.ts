// The list that opens above the input while typing `/command`, slash arguments, `@file`,
// or `!` shell commands and paths.

import { shallowRef } from 'vue'
import type { IconKey } from '../icons/generated/icons.ts'
import { RefError } from '../net/connection.ts'
import { fuzzyScore } from './fuzzy.ts'
import { mentionAt } from './mention.ts'

export const SLASH_PREFIX = '/'

export interface SlashCommand {
  name: string // canonical, without the slash
  aliases: string[]
  title: string
  requiresArgs: boolean
  custom: boolean // from the user's configuration; the built-in ones rank slightly higher
}

export interface ModelChoice {
  provider: string
  providerName: string
  id: string
  name: string
}

export interface AgentChoice {
  name: string
  description: string
}

export interface SessionChoice {
  id: string
  name: string
  title: string
  cwd: string
  live: boolean
}

export interface DirEntries {
  path: string
  dirs: string[]
  files: string[]
}

export type CompletionKind = 'slash' | 'file' | 'path' | 'arg' | 'shell'

export interface CompletionItem {
  value: string // what replaces the typed token
  label: string
  description: string
  icon?: IconKey
}

export interface Completion {
  kind: CompletionKind
  items: CompletionItem[]
  selected: number
  explicitlySelected: boolean
  freeformEnter?: boolean
  commitOnEnter?: boolean
  start: number // where the replaced token starts in the text
  end?: number // where the replaced token ends in the text (defaults to cursor)
}

export interface CompletionDeps {
  commands(): readonly SlashCommand[]
  listFiles(query: string): Promise<string[]>
  reportError(error: unknown): void
  sessionId?: string
  cwd?: () => string
  listDirs?: (path: string, options: { cwd?: string; includeFiles?: boolean }) => Promise<DirEntries>
  listBinaries?: (query: string, limit: number) => Promise<string[]>
  models?: () => readonly ModelChoice[]
  agents?: () => readonly AgentChoice[]
  variants?: () => readonly string[]
  sessions?: () => readonly SessionChoice[]
  liveSessions?: () => readonly SessionChoice[]
  shellHistory?: () => readonly string[]
}

export interface ShellContext {
  role: 'command' | 'arg'
  segmentIndex: number
  tokenStart: number
  token: string
  isPath: boolean
  atLineStart: boolean
}

interface PathRequestSpec {
  cacheKey: string
  cwd: string | undefined
  queryFolder: string
  rawFolder: string
  prefix: string
  separator: string
  includeFiles: boolean
  fileSpace: boolean
  defaultFile?: string
  start: number
  end: number
}

const MAX_ITEMS = 50
const MAX_FILES = 20
const MAX_BINARIES = 50
const MAX_DIR_CACHE = 64
export const FILE_QUERY_DELAY_MS = 80
const BUILTIN_BONUS = 5
const ENV_ASSIGNMENT = /^[A-Za-z_][A-Za-z0-9_]*=/

interface WrapperSpec {
  flagArgs: ReadonlySet<string>
  positionalArgs?: number
  noPositionalIfFlag?: ReadonlySet<string>
}

const WRAPPERS = new Map<string, WrapperSpec>([
  [
    'sudo',
    {
      flagArgs: new Set([
        '-C',
        '-c',
        '-D',
        '-g',
        '-h',
        '-p',
        '-R',
        '-r',
        '-t',
        '-T',
        '-U',
        '-u',
        '--close-from',
        '--login-class',
        '--chdir',
        '--group',
        '--host',
        '--prompt',
        '--chroot',
        '--role',
        '--type',
        '--command-timeout',
        '--other-user',
        '--user',
      ]),
    },
  ],
  ['doas', { flagArgs: new Set(['-a', '-C', '-u']) }],
  ['env', { flagArgs: new Set(['-a', '-C', '-S', '-u', '--argv0', '--chdir', '--split-string', '--unset']) }],
  ['nohup', { flagArgs: new Set() }],
  ['time', { flagArgs: new Set(['-f', '-o', '--format', '--output']) }],
  ['nice', { flagArgs: new Set(['-n', '--adjustment']) }],
  ['ionice', { flagArgs: new Set(['-c', '-n', '--class', '--classdata']) }],
  ['timeout', { flagArgs: new Set(['-k', '-s', '--kill-after', '--signal']), positionalArgs: 1 }],
  ['setsid', { flagArgs: new Set() }],
  [
    'strace',
    { flagArgs: new Set(['-a', '-b', '-e', '-E', '-I', '-o', '-O', '-p', '-P', '-s', '-S', '-u', '-U']) },
  ],
  [
    'ltrace',
    { flagArgs: new Set(['-A', '-a', '-D', '-e', '-F', '-l', '-n', '-o', '-p', '-s', '-S', '-u', '-x']) },
  ],
  ['chrt', { flagArgs: new Set(), positionalArgs: 1 }],
  [
    'taskset',
    {
      flagArgs: new Set(['-c', '--cpu-list']),
      positionalArgs: 1,
      noPositionalIfFlag: new Set(['-c', '--cpu-list', '-p', '--pid']),
    },
  ],
  ['stdbuf', { flagArgs: new Set(['-i', '-o', '-e', '--input', '--output', '--error']) }],
  ['watch', { flagArgs: new Set(['-n', '--interval']) }],
  ['command', { flagArgs: new Set() }],
  ['exec', { flagArgs: new Set(['-a']) }],
  ['builtin', { flagArgs: new Set() }],
  [
    'xargs',
    {
      flagArgs: new Set([
        '-a',
        '-d',
        '-E',
        '-e',
        '-I',
        '-i',
        '-L',
        '-l',
        '-n',
        '-P',
        '-s',
        '--arg-file',
        '--delimiter',
        '--eof',
        '--replace',
        '--max-lines',
        '--max-args',
        '--max-procs',
        '--max-chars',
        '--process-slot-var',
      ]),
    },
  ],
  ['caffeinate', { flagArgs: new Set(['-t', '-w']) }],
  ['pkexec', { flagArgs: new Set(['-u', '--user']) }],
])

interface SlashToken {
  start: number
  end: number
  query: string
}

function slashTokenAt(text: string, cursor: number): SlashToken | undefined {
  let tokenStart = cursor
  while (tokenStart > 0 && !/\s/.test(text[tokenStart - 1]!)) tokenStart--
  if (cursor <= tokenStart || text[tokenStart] !== SLASH_PREFIX) return undefined

  let end = cursor
  while (end < text.length && !/\s/.test(text[end]!)) end++

  const tokenBody = text.slice(tokenStart + 1, end)
  if (tokenBody.includes('/') || tokenBody.includes('\\')) return undefined

  return { start: tokenStart, end, query: text.slice(tokenStart + 1, cursor) }
}

export function slashItems(commands: readonly SlashCommand[], query: string): CompletionItem[] {
  const scored: { score: number; item: CompletionItem }[] = []
  for (const command of commands) {
    // The canonical name wins ties; an alias shows only when it matches better.
    let best: { score: number; name: string } | undefined
    for (const name of [command.name, ...command.aliases]) {
      const score = fuzzyScore(name, query)
      if (score !== undefined && (!best || score > best.score)) best = { score, name }
    }
    if (!best) continue

    const label =
      best.name === command.name
        ? `${SLASH_PREFIX}${command.name}`
        : `${SLASH_PREFIX}${best.name} → ${SLASH_PREFIX}${command.name}`
    const bonus = command.custom ? 0 : BUILTIN_BONUS
    const suffix = command.requiresArgs ? ' ' : ''
    scored.push({
      score: best.score + bonus,
      item: { value: `${SLASH_PREFIX}${command.name}${suffix}`, label, description: command.title },
    })
  }
  scored.sort((a, b) => b.score - a.score)
  return scored.slice(0, MAX_ITEMS).map((entry) => entry.item)
}

function fileItems(files: readonly string[]): CompletionItem[] {
  return files.map((file) => ({ value: `@${file} `, label: file, description: '' }))
}

function isDirNotFound(err: unknown): boolean {
  if (err instanceof RefError && (err.code === 'not_found' || err.code === 'dir_not_found')) return true
  if (!(err instanceof Error)) return false
  return err.message.startsWith('Directory does not exist:') || err.message.startsWith('Not a directory:')
}

function splitPathToken(raw: string): {
  rawFolder: string
  queryFolder: string
  prefix: string
  separator: string
} {
  const cut = Math.max(raw.lastIndexOf('/'), raw.lastIndexOf('\\'))
  const rawFolder = cut >= 0 ? raw.slice(0, cut + 1) : ''
  return {
    rawFolder,
    queryFolder: rawFolder || '.',
    prefix: cut >= 0 ? raw.slice(cut + 1) : raw,
    separator: cut >= 0 ? raw[cut]! : '/',
  }
}

function buildPathItems(spec: PathRequestSpec, entry: { dirs: string[]; files: string[] }): CompletionItem[] {
  const lowerPrefix = spec.prefix.toLowerCase()
  const showDot = spec.prefix.startsWith('.')
  const items: CompletionItem[] = []

  if (spec.rawFolder === '' && spec.prefix === '~') {
    items.push({ value: '~/', label: '~/', description: '' })
  }
  if (spec.rawFolder === '' && spec.prefix === '..') {
    items.push({ value: '../', label: '../', description: '' })
  }
  if (
    spec.includeFiles &&
    spec.defaultFile &&
    spec.rawFolder === '' &&
    spec.defaultFile.toLowerCase().startsWith(lowerPrefix) &&
    !entry.files.includes(spec.defaultFile)
  ) {
    items.push({ value: spec.defaultFile, label: spec.defaultFile, description: 'default' })
  }

  for (const dir of entry.dirs) {
    if (!showDot && dir.startsWith('.')) continue
    if (!dir.toLowerCase().startsWith(lowerPrefix)) continue
    items.push({
      value: `${spec.rawFolder}${dir}${spec.separator}`,
      label: `${spec.rawFolder}${dir}/`,
      description: '',
    })
  }

  if (spec.includeFiles) {
    const suffix = spec.fileSpace ? ' ' : ''
    for (const file of entry.files) {
      if (!showDot && file.startsWith('.')) continue
      if (!file.toLowerCase().startsWith(lowerPrefix)) continue
      items.push({
        value: `${spec.rawFolder}${file}${suffix}`,
        label: `${spec.rawFolder}${file}`,
        description: '',
      })
    }
  }

  return items.slice(0, MAX_ITEMS)
}

function splitShellWords(input: string): string[] {
  const words: string[] = []
  let current = ''
  let quote: "'" | '"' | null = null
  let hasToken = false

  for (let i = 0; i < input.length; i++) {
    const ch = input[i]!
    if (quote) {
      if (ch === '\\' && quote === '"' && i + 1 < input.length) current += input[++i]!
      else if (ch === quote) quote = null
      else current += ch
      hasToken = true
      continue
    }
    if (ch === "'" || ch === '"') {
      quote = ch
      hasToken = true
      continue
    }
    if (ch === '\\' && i + 1 < input.length) {
      current += input[++i]!
      hasToken = true
      continue
    }
    if (/\s/.test(ch)) {
      if (hasToken) {
        words.push(current)
        current = ''
        hasToken = false
      }
      continue
    }
    current += ch
    hasToken = true
  }
  if (hasToken) words.push(current)
  return words
}

function wrapperKey(word: string): string {
  const cut = Math.max(word.lastIndexOf('/'), word.lastIndexOf('\\'))
  return cut >= 0 ? word.slice(cut + 1) : word
}

function flagName(word: string): string {
  const eq = word.indexOf('=')
  return eq >= 0 ? word.slice(0, eq) : word
}

function clearsPositional(flags: ReadonlySet<string>, word: string): boolean {
  if (flags.has(flagName(word))) return true
  if (!word.startsWith('-') || word.startsWith('--') || word.includes('=')) return false
  return [...word.slice(1)].some((ch) => flags.has(`-${ch}`))
}

function flagConsumesNextWord(spec: WrapperSpec, word: string): boolean {
  if (spec.flagArgs.has(word)) return true
  if (!word.startsWith('-') || word.startsWith('--') || word.length <= 2 || word.includes('=')) {
    return false
  }
  for (let i = 1; i < word.length; i++) {
    if (spec.flagArgs.has(`-${word[i]!}`)) return i === word.length - 1
  }
  return false
}

function classifyShellRole(priorWords: readonly string[]): 'command' | 'wrapper-arg' | 'arg' {
  let activeWrapper: WrapperSpec | undefined
  let expectFlagArg = false
  let endOfOptions = false
  let remainingPositional = 0

  for (const word of priorWords) {
    if (expectFlagArg) {
      expectFlagArg = false
      continue
    }
    if (activeWrapper) {
      if (!endOfOptions && word === '--') {
        endOfOptions = true
        continue
      }
      if (!endOfOptions && word.startsWith('-')) {
        if (activeWrapper.noPositionalIfFlag && clearsPositional(activeWrapper.noPositionalIfFlag, word)) {
          remainingPositional = 0
        }
        if (flagConsumesNextWord(activeWrapper, word)) expectFlagArg = true
        continue
      }
      if (ENV_ASSIGNMENT.test(word) && remainingPositional === 0) continue
      if (remainingPositional > 0) {
        remainingPositional--
        continue
      }
      activeWrapper = undefined
      endOfOptions = false
    }
    if (ENV_ASSIGNMENT.test(word)) continue
    const spec = WRAPPERS.get(wrapperKey(word))
    if (spec) {
      activeWrapper = spec
      endOfOptions = false
      remainingPositional = spec.positionalArgs ?? 0
      continue
    }
    return 'arg'
  }
  if (expectFlagArg || remainingPositional > 0) return 'wrapper-arg'
  return 'command'
}

export function parseShellContext(text: string, cursor: number): ShellContext | undefined {
  if (text === '') return undefined
  const before = text.slice(0, cursor)
  let quote: "'" | '"' | null = null
  let segmentStart = 0
  let segmentIndex = 0

  for (let i = 0; i < before.length; i++) {
    const ch = before[i]!
    if (quote) {
      if (ch === '\\' && quote === '"' && i + 1 < before.length) i++
      else if (ch === quote) quote = null
      continue
    }
    if (ch === "'" || ch === '"') {
      quote = ch
      continue
    }
    if (ch === '\\' && i + 1 < before.length) {
      i++
      continue
    }
    if (ch === ';') {
      segmentStart = i + 1
      segmentIndex++
      continue
    }
    if (ch === '|') {
      if (before[i + 1] === '|') i++
      segmentStart = i + 1
      segmentIndex++
      continue
    }
    if (ch === '&' && before[i + 1] === '&') {
      i++
      segmentStart = i + 1
      segmentIndex++
    }
  }

  const segment = before.slice(segmentStart)
  const tokenMatch = /(\S*)$/.exec(segment)!
  const token = tokenMatch[1]!
  const tokenStart = cursor - token.length
  const atLineStart = segmentIndex === 0 && tokenStart === 0
  const prior = segment.slice(0, segment.length - token.length).trim()
  const priorWords = prior === '' ? [] : splitShellWords(prior)
  const role = classifyShellRole(priorWords)

  if (role === 'wrapper-arg') return undefined

  if (role === 'command') {
    if (token === '' || token.startsWith('-') || ENV_ASSIGNMENT.test(token)) return undefined
    const isPath =
      token.includes('/') || token.includes('\\') || token.startsWith('.') || token.startsWith('~')
    return { role: 'command', segmentIndex, tokenStart, token, isPath, atLineStart }
  }

  if (token.startsWith('-')) return undefined
  return { role: 'arg', segmentIndex, tokenStart, token, isPath: true, atLineStart }
}

function rankEnumItems(
  entries: readonly { keys: readonly string[]; item: CompletionItem }[],
  query: string,
): CompletionItem[] {
  if (query === '') return entries.slice(0, MAX_ITEMS).map((entry) => entry.item)
  const scored: { score: number; item: CompletionItem }[] = []
  for (const entry of entries) {
    let best: number | undefined
    for (const key of entry.keys) {
      const score = fuzzyScore(key, query)
      if (score !== undefined && (best === undefined || score > best)) best = score
    }
    if (best !== undefined) scored.push({ score: best, item: entry.item })
  }
  scored.sort((a, b) => b.score - a.score)
  return scored.slice(0, MAX_ITEMS).map((entry) => entry.item)
}

export class CompletionController {
  private readonly deps: CompletionDeps
  private readonly current = shallowRef<Completion | undefined>() // replaced, never edited
  private suppressed = false // Esc closed the list; it stays closed until the text changes
  private lastInput: { text: string; cursor: number; shell: boolean } | undefined
  private slashQuery: string | undefined
  private fileTimer: ReturnType<typeof setTimeout> | undefined
  private filesInFlight = false
  private wantedQuery: string | undefined
  private mention: { start: number; query: string } | undefined

  private readonly dirCache = new Map<string, { dirs: string[]; files: string[] }>()
  private pathTimer: ReturnType<typeof setTimeout> | undefined
  private activePathSpec: PathRequestSpec | undefined
  private pathFetchSeq = 0

  private binaryTimer: ReturnType<typeof setTimeout> | undefined
  private activeBinaryQuery: { query: string; start: number; end: number; segmentIndex: number } | undefined
  private binaryFetchSeq = 0

  constructor(deps: CompletionDeps) {
    this.deps = deps
  }

  get completion(): Completion | undefined {
    return this.current.value
  }

  get selectedItem(): CompletionItem | undefined {
    const completion = this.current.value
    return completion?.items[completion.selected]
  }

  invalidateDirs(): void {
    this.dirCache.clear()
  }

  // Call after every change of the text or the cursor. `edited`: the text changed (not only the cursor).
  update(text: string, cursor: number, shell: boolean, edited: boolean): void {
    const unchanged =
      !edited &&
      this.lastInput !== undefined &&
      this.lastInput.text === text &&
      this.lastInput.cursor === cursor &&
      this.lastInput.shell === shell
    if (unchanged) return
    this.lastInput = { text, cursor, shell }

    if (edited) this.suppressed = false
    this.cancelTimers()
    if (this.suppressed) return this.close()

    if (shell) {
      this.updateShell(text, cursor, edited)
      return
    }

    if (text.startsWith(SLASH_PREFIX) && this.trySlashArgument(text, cursor, edited)) return

    const slash = slashTokenAt(text, cursor)
    if (slash) {
      this.showCommands(slash.query, slash.start, slash.end)
      return
    }
    this.mention = mentionAt(text, cursor)
    if (!this.mention) return this.close()
    if (this.current.value?.kind !== 'file') this.close()
    const { query: fileQuery } = this.mention
    this.fileTimer = setTimeout(() => this.requestFiles(fileQuery), FILE_QUERY_DELAY_MS)
  }

  move(delta: number): void {
    const completion = this.current.value
    if (!completion || completion.items.length === 0) return
    const count = completion.items.length
    const selected = (((completion.selected + delta) % count) + count) % count
    this.current.value = { ...completion, selected, explicitlySelected: true }
  }

  select(index: number): void {
    const completion = this.current.value
    if (!completion || index === completion.selected || !completion.items[index]) return
    this.current.value = { ...completion, selected: index }
  }

  // Esc: close the list, and keep it closed while the text stays the same.
  dismiss(): void {
    this.suppressed = true
    this.cancelTimers()
    this.close()
  }

  close(): void {
    this.current.value = undefined
    this.slashQuery = undefined
    this.activePathSpec = undefined
    this.activeBinaryQuery = undefined
  }

  private cancelTimers(): void {
    clearTimeout(this.fileTimer)
    clearTimeout(this.pathTimer)
    clearTimeout(this.binaryTimer)
    this.mention = undefined
    this.activePathSpec = undefined
    this.activeBinaryQuery = undefined
  }

  private trySlashArgument(text: string, cursor: number, edited: boolean): boolean {
    const header = /^\/(\S+)\s+/.exec(text)
    if (!header || cursor < header[0].length) return false
    const typed = header[1]!.toLowerCase()
    const command = this.deps
      .commands()
      .find(
        (candidate) =>
          candidate.name.toLowerCase() === typed ||
          candidate.aliases.some((alias) => alias.toLowerCase() === typed),
      )
    if (!command || command.custom) return false

    const argStart = header[0].length
    const argText = text.slice(argStart, cursor)

    switch (command.name) {
      case 'cd':
        this.requestPathCompletion(
          argText,
          argStart,
          cursor,
          { includeFiles: false, fileSpace: false },
          edited,
        )
        return true
      case 'export': {
        const defaultFile = this.deps.sessionId ? `ref-session-${this.deps.sessionId}.md` : undefined
        this.requestPathCompletion(
          argText,
          argStart,
          cursor,
          { includeFiles: true, fileSpace: false, defaultFile },
          edited,
        )
        return true
      }
      case 'models':
        this.showModelsArg(argText.trim(), argStart, text.length)
        return true
      case 'agents':
        this.showAgentsArg(argText.trim(), argStart, text.length)
        return true
      case 'variant':
        this.showVariantsArg(argText.trim(), argStart, text.length)
        return true
      case 'sessions':
        this.showSessionsArg(argText.trim(), argStart, text.length)
        return true
      case 'close-session':
        this.showCloseSessionArg(argText, cursor)
        return true
      default:
        return false
    }
  }

  private showModelsArg(query: string, start: number, end: number): void {
    const models = this.deps.models?.() ?? []
    const entries = models.map((model) => {
      const ref = `${model.provider}/${model.id}`
      return {
        keys: [ref, model.id, model.name],
        item: {
          value: ref,
          label: ref,
          description: model.name !== model.id ? model.name : model.providerName,
          icon: 'model-16' as const,
        },
      }
    })
    this.applyEnumCompletion(rankEnumItems(entries, query), start, end, { commitOnEnter: true })
  }

  private showAgentsArg(query: string, start: number, end: number): void {
    const agents = this.deps.agents?.() ?? []
    const entries = agents.map((agent) => ({
      keys: [agent.name, agent.description],
      item: {
        value: agent.name,
        label: agent.name,
        description: agent.description,
        icon: 'agent-16' as const,
      },
    }))
    this.applyEnumCompletion(rankEnumItems(entries, query), start, end, { commitOnEnter: true })
  }

  private showVariantsArg(query: string, start: number, end: number): void {
    const raw = ['default', ...(this.deps.variants?.() ?? [])]
    const unique = [...new Set(raw)]
    const entries = unique.map((name) => ({
      keys: [name],
      item: {
        value: name,
        label: name,
        description: '',
        icon: 'variant-16' as const,
      },
    }))
    this.applyEnumCompletion(rankEnumItems(entries, query), start, end, { commitOnEnter: true })
  }

  private showSessionsArg(query: string, start: number, end: number): void {
    const sessions = this.deps.sessions?.() ?? []
    const nameCounts = new Map<string, number>()
    for (const session of sessions) {
      if (!session.name) continue
      const key = session.name.toLowerCase()
      nameCounts.set(key, (nameCounts.get(key) ?? 0) + 1)
    }
    const entries = sessions.map((session) => {
      const duplicateName = session.name !== '' && (nameCounts.get(session.name.toLowerCase()) ?? 0) > 1
      const value = !session.name || duplicateName ? session.id : session.name
      const label = duplicateName ? `${session.name} (${session.id})` : session.name || session.id
      return {
        keys: [session.name, session.id, session.title].filter(Boolean),
        item: {
          value,
          label,
          description: session.title || session.cwd,
          icon: 'chat-16' as const,
        },
      }
    })
    this.applyEnumCompletion(rankEnumItems(entries, query), start, end, { commitOnEnter: true })
  }

  private showCloseSessionArg(argText: string, cursor: number): void {
    const live = this.deps.liveSessions?.() ?? []
    const tokenMatch = /(\S*)$/.exec(argText)!
    const token = tokenMatch[1]!
    const tokenStart = cursor - token.length
    const priorPart = argText.slice(0, argText.length - token.length).trim()
    const alreadyChosen = new Set(priorPart === '' ? [] : priorPart.split(/\s+/).map((s) => s.toLowerCase()))

    const entries = live
      .filter(
        (session) =>
          !alreadyChosen.has(session.id.toLowerCase()) &&
          (session.name === '' || !alreadyChosen.has(session.name.toLowerCase())),
      )
      .map((session) => {
        const tokenValue = session.name && !/\s/.test(session.name) ? session.name : session.id
        return {
          keys: [session.name, session.id, session.title].filter(Boolean),
          item: {
            value: `${tokenValue} `,
            label: tokenValue,
            description: session.title || session.cwd,
            icon: 'chat-16' as const,
          },
        }
      })

    const hasPrior = alreadyChosen.size > 0
    const freeformEnter = hasPrior && token === ''
    this.applyEnumCompletion(rankEnumItems(entries, token), tokenStart, cursor, {
      freeformEnter,
      commitOnEnter: !freeformEnter,
    })
  }

  private applyEnumCompletion(
    items: CompletionItem[],
    start: number,
    end: number,
    options: { freeformEnter?: boolean; commitOnEnter?: boolean },
  ): void {
    if (items.length === 0) return this.close()
    this.current.value = {
      kind: 'arg',
      items,
      selected: 0,
      explicitlySelected: false,
      freeformEnter: options.freeformEnter,
      commitOnEnter: options.commitOnEnter,
      start,
      end,
    }
  }

  private updateShell(text: string, cursor: number, edited: boolean): void {
    const context = parseShellContext(text, cursor)
    if (!context) return this.close()

    if (context.role === 'arg' || context.isPath) {
      this.requestPathCompletion(
        context.token,
        context.tokenStart,
        cursor,
        { includeFiles: true, fileSpace: true },
        edited,
      )
      return
    }

    this.requestShellCommandCompletion(
      context.token,
      context.tokenStart,
      cursor,
      context.segmentIndex,
      context.atLineStart,
    )
  }

  private requestPathCompletion(
    rawToken: string,
    start: number,
    end: number,
    options: { includeFiles: boolean; fileSpace: boolean; defaultFile?: string },
    edited: boolean,
  ): void {
    if (!this.deps.listDirs) return this.close()
    const cwd = this.deps.cwd?.()
    const split = splitPathToken(rawToken)
    const cacheKey = `${cwd ?? ''}::${options.includeFiles ? 'all' : 'dirs'}::${split.queryFolder}`
    const spec: PathRequestSpec = {
      cacheKey,
      cwd,
      queryFolder: split.queryFolder,
      rawFolder: split.rawFolder,
      prefix: split.prefix,
      separator: split.separator,
      includeFiles: options.includeFiles,
      fileSpace: options.fileSpace,
      defaultFile: options.defaultFile,
      start,
      end,
    }
    this.activePathSpec = spec

    const cached = this.dirCache.get(cacheKey)
    if (cached) {
      this.showPathItems(spec, cached)
      return
    }

    const immediate = edited && split.rawFolder.endsWith(split.separator)
    if (immediate) {
      this.fetchPathEntries(spec)
      return
    }
    this.pathTimer = setTimeout(() => this.fetchPathEntries(spec), FILE_QUERY_DELAY_MS)
  }

  private fetchPathEntries(spec: PathRequestSpec): void {
    const listDirs = this.deps.listDirs
    if (!listDirs) return
    const seq = ++this.pathFetchSeq
    listDirs(spec.queryFolder, { cwd: spec.cwd, includeFiles: spec.includeFiles })
      .then((res) => {
        const entry = { dirs: res.dirs, files: res.files ?? [] }
        if (this.dirCache.size >= MAX_DIR_CACHE) {
          const oldest = this.dirCache.keys().next().value
          if (oldest !== undefined) this.dirCache.delete(oldest)
        }
        this.dirCache.set(spec.cacheKey, entry)
        if (seq !== this.pathFetchSeq || this.activePathSpec?.cacheKey !== spec.cacheKey) return
        this.showPathItems(this.activePathSpec, entry)
      })
      .catch((error) => {
        if (seq !== this.pathFetchSeq) return
        if (isDirNotFound(error)) {
          if (this.activePathSpec?.cacheKey === spec.cacheKey) {
            this.showPathItems(this.activePathSpec, { dirs: [], files: [] })
          }
          return
        }
        this.deps.reportError(error)
      })
  }

  private showPathItems(spec: PathRequestSpec, entry: { dirs: string[]; files: string[] }): void {
    const items = buildPathItems(spec, entry)
    if (items.length === 0) return this.close()
    this.current.value = {
      kind: 'path',
      items,
      selected: 0,
      explicitlySelected: false,
      freeformEnter: true,
      start: spec.start,
      end: spec.end,
    }
  }

  private shellHistoryItems(query: string, atLineStart: boolean): CompletionItem[] {
    if (!atLineStart) return []
    const entries = this.deps.shellHistory?.() ?? []
    const lower = query.toLowerCase()
    const seen = new Set<string>()
    const out: CompletionItem[] = []

    for (let i = entries.length - 1; i >= 0; i--) {
      const raw = entries[i]!
      if (!raw.startsWith('!')) continue
      const cmd = raw.slice(1).trim()
      if (cmd === '' || seen.has(cmd)) continue
      if (!cmd.toLowerCase().startsWith(lower)) continue
      seen.add(cmd)
      out.push({ value: cmd, label: cmd, description: 'history', icon: 'history-16' })
      if (out.length >= 8) break
    }
    return out
  }

  private requestShellCommandCompletion(
    query: string,
    start: number,
    end: number,
    segmentIndex: number,
    atLineStart: boolean,
  ): void {
    const historyItems = this.shellHistoryItems(query, atLineStart)
    this.activeBinaryQuery = { query, start, end, segmentIndex }
    if (historyItems.length > 0) {
      this.current.value = {
        kind: 'shell',
        items: historyItems,
        selected: 0,
        explicitlySelected: false,
        freeformEnter: true,
        start,
        end,
      }
    } else if (this.current.value?.kind !== 'shell' || this.current.value.start !== start) {
      this.current.value = undefined
    }

    if (!this.deps.listBinaries) {
      if (historyItems.length === 0) this.close()
      return
    }

    const seq = ++this.binaryFetchSeq
    this.binaryTimer = setTimeout(() => {
      this.deps.listBinaries!(query, MAX_BINARIES)
        .then((binaries) => {
          if (seq !== this.binaryFetchSeq || this.activeBinaryQuery?.query !== query) return
          this.showShellCommandItems(historyItems, binaries, start, end)
        })
        .catch((error) => this.deps.reportError(error))
    }, FILE_QUERY_DELAY_MS)
  }

  private showShellCommandItems(
    historyItems: readonly CompletionItem[],
    binaries: readonly string[],
    start: number,
    end: number,
  ): void {
    const seen = new Set(historyItems.map((item) => item.label))
    const binItems: CompletionItem[] = []
    for (const bin of binaries) {
      if (seen.has(bin)) continue
      seen.add(bin)
      binItems.push({ value: `${bin} `, label: bin, description: '', icon: 'terminal-16' })
    }
    const items = [...historyItems, ...binItems].slice(0, MAX_ITEMS)
    if (items.length === 0) return this.close()

    const prev = this.current.value
    const preserve = prev?.kind === 'shell' && prev.explicitlySelected
    this.current.value = {
      kind: 'shell',
      items,
      selected: preserve ? Math.min(prev.selected, items.length - 1) : 0,
      explicitlySelected: preserve ?? false,
      freeformEnter: true,
      start,
      end,
    }
  }

  private showCommands(query: string, start: number, end: number): void {
    const items = slashItems(this.deps.commands(), query)
    if (items.length === 0) return this.close()

    const previous = this.current.value
    const sameToken = previous?.kind === 'slash' && previous.start === start && this.slashQuery === query
    const keep = sameToken ? previous.selected : 0
    this.slashQuery = query
    this.current.value = {
      kind: 'slash',
      items,
      selected: Math.min(keep, items.length - 1),
      explicitlySelected: sameToken ? previous.explicitlySelected : false,
      start,
      end,
    }
  }

  private requestFiles(query: string): void {
    if (this.filesInFlight) {
      this.wantedQuery = query
      return
    }
    this.filesInFlight = true
    this.deps
      .listFiles(query)
      .then((files) => this.showFiles(query, files))
      .catch((error) => this.deps.reportError(error))
      .finally(() => this.fetched(query))
  }

  private fetched(query: string): void {
    this.filesInFlight = false
    const wanted = this.wantedQuery
    this.wantedQuery = undefined
    if (wanted !== undefined && wanted !== query) this.requestFiles(wanted)
  }

  private showFiles(query: string, files: string[]): void {
    const mention = this.mention
    const stale = !mention || mention.query !== query
    if (stale) return

    const items = fileItems(files.slice(0, MAX_FILES))
    if (items.length === 0) return this.close()

    const previous = this.current.value
    const keep = previous?.kind === 'file' ? previous.selected : 0
    this.current.value = {
      kind: 'file',
      items,
      selected: Math.min(keep, items.length - 1),
      explicitlySelected: previous?.kind === 'file' ? previous.explicitlySelected : false,
      start: mention.start,
    }
  }
}
