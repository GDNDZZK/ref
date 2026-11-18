// The input box of one session: draft, history, completion list, sending and taking back.
// A ChatSession owns one, so the draft survives switching to another session and back.
//
//   keys / paste ---> SessionComposer ---> Prompt | Shell | slash command | ExpandCommand
//   Undo / RemoveQueued ---> restore() ---> back into the draft

import { reactive, ref } from 'vue'
import type { RefApi } from '../net/api.ts'
import type { NamedImage, ToastLevel } from '../proto/generated'
import { CompletionController, SLASH_PREFIX } from './completion.ts'
import type { AgentChoice, ModelChoice, SessionChoice, SlashCommand } from './completion.ts'
import {
  attachImage,
  clearDraft,
  dropOrphanAttachments,
  emptyDraft,
  insertToken,
  restoreInto,
  submissionOf,
} from './draft.ts'
import type { Draft } from './draft.ts'
import { onDraftRestored } from './draftBus.ts'
import { HistoryBrowser } from './history.ts'
import type { PromptHistory } from './history.ts'
import { readImage, rejectionOf } from './images.ts'

export type ComposerApi = Pick<RefApi, 'prompt' | 'shell' | 'removeQueued' | 'listFiles' | 'expandCommand'> &
  Partial<Pick<RefApi, 'listDirs' | 'listBinaries'>>

export interface ComposerHost {
  sessionId: string
  api(): ComposerApi
  history: { history: PromptHistory; push(text: string): void }
  cwd(): string
  isBusy(): boolean
  hasDelayedTool(): boolean // the last tool call waits out its delay: one Esc cancels it
  slashCommands(): readonly SlashCommand[]
  models?(): readonly ModelChoice[]
  agents?(): readonly AgentChoice[]
  variants?(): readonly string[]
  sessions?(): readonly SessionChoice[]
  liveSessions?(): readonly SessionChoice[]
  runSlash(line: string): Promise<void> // a built-in command, e.g. "/rename Work"
  abort(): Promise<void> // stops the running turn
  toast(level: ToastLevel, messageKey: string, params?: Record<string, string>): void
  reportError(error: unknown): void
}

export const INTERRUPT_ARMED_MS = 1500
const SHELL_PREFIX = '!'
const FILE_LIMIT = 20
const MIN_CHARS_TO_KEEP = 20 // a cleared text this long is kept in the history
const SINGLE_SLASH_TOKEN = /^\/([A-Za-z][A-Za-z0-9_-]*)$/

function damerauDistance(a: string, b: string): number {
  const m = a.length
  const n = b.length
  let prev = Array.from({ length: n + 1 }, (_, j) => j)
  let prevPrev: number[] | undefined

  for (let i = 1; i <= m; i++) {
    const curr = [i]
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1
      let best = Math.min(prev[j]! + 1, curr[j - 1]! + 1, prev[j - 1]! + cost)
      if (prevPrev !== undefined && i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) {
        best = Math.min(best, prevPrev[j - 2]! + 1)
      }
      curr.push(best)
    }
    prevPrev = prev
    prev = curr
  }
  return prev[n]!
}

export function suggestSlashCommand(typed: string, commands: readonly SlashCommand[]): string | undefined {
  const target = typed.toLowerCase()
  let maxDist = 3
  if (target.length <= 3) maxDist = 1
  else if (target.length <= 6) maxDist = 2

  let best: { score: number; name: string } | undefined
  for (const command of commands) {
    for (const candidate of [command.name, ...command.aliases]) {
      const lower = candidate.toLowerCase()
      const isPrefix = target.length >= 2 && lower.startsWith(target) && lower.length - target.length <= 6
      const dist = damerauDistance(target, lower)
      if (!isPrefix && dist > maxDist) continue
      const score = isPrefix ? 0.5 + (lower.length - target.length) * 0.05 : dist
      if (!best || score < best.score) best = { score, name: candidate }
    }
  }
  return best?.name
}
export class SessionComposer {
  readonly draft: Draft = reactive(emptyDraft())
  readonly interruptArmed = ref(false) // Esc was pressed once; a second one within INTERRUPT_ARMED_MS aborts
  readonly removingQueued = reactive(new Set<string>()) // message ids being taken back
  readonly focusRequests = ref(0) // counts the times the box was asked to take the focus

  private readonly host: ComposerHost
  private readonly completionController: CompletionController
  private readonly browser: HistoryBrowser
  private cursor = 0
  private disarmTimer: ReturnType<typeof setTimeout> | undefined
  private readonly stopListening: () => void

  constructor(host: ComposerHost) {
    this.host = host
    this.browser = new HistoryBrowser(host.history.history)
    this.completionController = new CompletionController({
      sessionId: host.sessionId,
      cwd: () => host.cwd(),
      commands: () => host.slashCommands(),
      listFiles: (query) => host.api().listFiles(host.cwd(), query, FILE_LIMIT),
      listDirs: (path, options) => {
        const fn = host.api().listDirs
        return fn ? fn(path, options) : Promise.resolve({ path, dirs: [], files: [] })
      },
      listBinaries: (query, limit) => {
        const fn = host.api().listBinaries
        return fn ? fn(query, limit) : Promise.resolve([])
      },
      models: () => host.models?.() ?? [],
      agents: () => host.agents?.() ?? [],
      variants: () => host.variants?.() ?? [],
      sessions: () => host.sessions?.() ?? [],
      liveSessions: () => host.liveSessions?.() ?? [],
      shellHistory: () => host.history.history.entries,
      reportError: (error) => host.reportError(error),
    })
    this.stopListening = onDraftRestored((sessionId, text, images) => {
      if (sessionId === host.sessionId) this.restore(text, images)
    })
  }

  get completion() {
    return this.completionController.completion
  }

  // Where the caret is (or should be after the composer changed the text).
  get caret(): number {
    return this.cursor
  }

  requestFocus(): void {
    this.focusRequests.value++
  }

  dispose(): void {
    this.stopListening()
    clearTimeout(this.disarmTimer)
  }

  // --- text -----------------------------------------------------------------------------

  // The text or the caret changed in the box.
  input(text: string, cursor: number): void {
    const edited = text !== this.draft.text
    this.cursor = cursor
    if (edited) {
      this.draft.text = text
      this.browser.reset()
      dropOrphanAttachments(this.draft)
    }
    this.completionController.update(this.draft.text, cursor, this.draft.shell, edited)
  }

  // Replaces the text and puts the caret at its end; returns the caret.
  private setText(text: string): number {
    this.draft.text = text
    this.cursor = text.length
    this.completionController.update(text, this.cursor, this.draft.shell, true)
    return this.cursor
  }

  // `!` typed into an empty box switches to shell mode; Backspace on the empty box leaves it.
  enterShell(): void {
    this.draft.shell = true
    this.browser.reset()
    this.completionController.close()
  }

  leaveShell(): void {
    this.draft.shell = false
    this.browser.reset()
    this.completionController.close()
  }

  // --- history --------------------------------------------------------------------------

  // ↑ with the caret at the start (or in an empty box). Returns the new caret when it acted.
  historyPrev(): number | undefined {
    const isShell = this.draft.shell
    const current = isShell ? SHELL_PREFIX + this.draft.text : this.draft.text
    const filter = isShell ? (entry: string) => entry.startsWith(SHELL_PREFIX) : undefined
    const entry = this.browser.prev(current, filter)
    return entry === undefined ? undefined : this.showHistoryEntry(entry)
  }

  // ↓ with the caret at the end while browsing.
  historyNext(): number | undefined {
    const entry = this.browser.next()
    return entry === undefined ? undefined : this.showHistoryEntry(entry)
  }

  get browsingHistory(): boolean {
    return this.browser.browsing
  }

  private showHistoryEntry(entry: string): number {
    const shell = entry.startsWith(SHELL_PREFIX)
    this.draft.shell = shell
    this.completionController.close()
    this.draft.text = shell ? entry.slice(SHELL_PREFIX.length) : entry
    this.cursor = this.draft.text.length
    return this.cursor
  }

  // --- completion -----------------------------------------------------------------------

  moveCompletion(delta: number): void {
    this.completionController.move(delta)
  }

  selectCompletion(index: number): void {
    this.completionController.select(index)
  }

  // Tab inserts the choice; Enter (`run`) also runs a command that needs no arguments (or enum arg).
  // Returns the new caret when the text changed.
  acceptCompletion(run: boolean): number | undefined {
    const completion = this.completionController.completion
    const item = this.completionController.selectedItem
    if (!completion || !item) return undefined
    this.completionController.close()

    if (completion.kind === 'slash') {
      return this.acceptCommand(item.value, run)
    }

    const text = this.draft.text
    const end = completion.end ?? this.cursor
    const replaced = text.slice(0, completion.start) + item.value + text.slice(end)
    this.cursor = completion.start + item.value.length
    this.draft.text = replaced

    if (run && completion.commitOnEnter) {
      void this.submit()
      return undefined
    }

    this.completionController.update(replaced, this.cursor, this.draft.shell, true)
    return this.cursor
  }

  private acceptCommand(value: string, run: boolean): number | undefined {
    const name = value.trim().slice(SLASH_PREFIX.length)
    const commands = this.host.slashCommands()
    const takesArgs = commands.find((command) => command.name === name)?.takesArgs ?? true

    const typed = this.draft.text.trim().replace(/^\/+/, '').toLowerCase()
    const typedCommand = commands.find(
      (c) => c.name.toLowerCase() === typed || c.aliases.some((alias) => alias.toLowerCase() === typed),
    )
    const typedFully = (typedCommand?.name ?? typed).toLowerCase() === name.toLowerCase()
    if (run && takesArgs && typedFully) {
      this.draft.text = `${SLASH_PREFIX}${name}`
      void this.submit()
      return undefined
    }
    const command = commands.find((candidate) => candidate.name === name)
    if (run && !takesArgs && command) {
      this.host.history.push(`${SLASH_PREFIX}${name}`)
      clearDraft(this.draft)
      void this.runCommand(command, `${SLASH_PREFIX}${name}`)
      return undefined
    }
    return this.setText(value)
  }

  // The command a line such as "/rename Work" names, if there is one.
  private commandIn(line: string): SlashCommand | undefined {
    if (!line.startsWith(SLASH_PREFIX)) return undefined
    const words = line.slice(SLASH_PREFIX.length).split(/\s+/) as [string, ...string[]]
    const typed = words[0].toLowerCase()
    return this.host
      .slashCommands()
      .find(
        (command) =>
          command.name.toLowerCase() === typed ||
          command.aliases.some((alias) => alias.toLowerCase() === typed),
      )
  }

  private async runCommand(command: SlashCommand, line: string): Promise<void> {
    if (command.custom) {
      const args = line.replace(/^\/\S+\s*/, '')
      await this.runCustomCommand(command.name, args)
      return
    }
    try {
      await this.host.runSlash(line)
    } catch (error) {
      this.host.reportError(error)
    }
  }

  // The x on a thumbnail: the token goes from the text (with the space after it), and the image with it.
  removeAttachment(token: string): void {
    this.draft.text = this.draft.text.replace(`${token} `, '').replace(token, '')
    this.cursor = Math.min(this.cursor, this.draft.text.length)
    dropOrphanAttachments(this.draft)
  }

  // Ctrl+C: empties the box. A long text goes to the history first, so it is not lost.
  clear(): void {
    const text = this.draft.text.trim()
    if (Array.from(text).length >= MIN_CHARS_TO_KEEP) this.host.history.push(this.draft.text)
    clearDraft(this.draft)
    this.browser.reset()
    this.cursor = 0
    this.completionController.close()
  }

  // --- sending --------------------------------------------------------------------------

  async submit(): Promise<void> {
    const draft = this.draft
    const trimmed = draft.text.trim()
    if (trimmed === '' && draft.attachments.length === 0) return
    this.completionController.close()
    this.completionController.invalidateDirs()
    this.browser.reset()

    if (draft.shell) return this.submitShell(trimmed)
    const command = this.commandIn(trimmed)
    if (!command) {
      const singleSlash = SINGLE_SLASH_TOKEN.exec(trimmed)
      if (singleSlash && draft.attachments.length === 0) {
        const unknownName = singleSlash[1]!
        const suggestion = suggestSlashCommand(unknownName, this.host.slashCommands())
        if (suggestion) {
          this.host.toast('warn', 'composer.toast.unknownCommandSuggest', {
            name: unknownName,
            suggestion,
          })
        } else {
          this.host.toast('warn', 'composer.toast.unknownCommand', { name: unknownName })
        }
        return
      }
      return this.submitPrompt()
    }

    this.host.history.push(trimmed)
    clearDraft(draft)
    await this.runCommand(command, trimmed)
  }

  private async submitShell(command: string): Promise<void> {
    this.host.history.push(SHELL_PREFIX + command)
    clearDraft(this.draft)
    try {
      await this.host.api().shell(this.host.sessionId, command)
    } catch (error) {
      this.draft.shell = true
      this.setText(command)
      this.host.reportError(error)
    }
  }

  private async submitPrompt(): Promise<void> {
    const { text, images } = submissionOf(this.draft)
    this.host.history.push(text)
    clearDraft(this.draft)
    await this.sendPrompt(text, images)
  }

  // On failure the prompt goes back into the box, so nothing typed is lost.
  private async sendPrompt(text: string, images: NamedImage[]): Promise<void> {
    try {
      await this.host.api().prompt(this.host.sessionId, text, images)
    } catch (error) {
      this.restore(text, images)
      this.host.reportError(error)
    }
  }

  // Runs a command from the user's configuration: the server expands it, the result is sent.
  async runCustomCommand(name: string, args: string): Promise<void> {
    try {
      const text = await this.host.api().expandCommand(name, args, this.host.cwd())
      if (text.trim() === '') {
        this.host.toast('warn', 'composer.toast.expandedToNothing')
        return
      }
      await this.sendPrompt(text, [])
    } catch (error) {
      this.host.reportError(error)
    }
  }

  // --- interrupting ---------------------------------------------------------------------

  // Esc, in the order of what it closes: the list, shell mode, a delayed tool call, the running turn
  // (that one needs two presses within INTERRUPT_ARMED_MS).
  escape(): void {
    if (this.completionController.completion) return this.completionController.dismiss()
    if (this.draft.shell) return this.leaveShell()
    if (this.host.hasDelayedTool()) {
      this.host.toast('info', 'composer.toast.delayedCancelled')
      void this.abort()
      return
    }
    if (!this.host.isBusy()) return
    if (this.interruptArmed.value) return this.abortArmed()
    this.interruptArmed.value = true
    this.disarmTimer = setTimeout(() => (this.interruptArmed.value = false), INTERRUPT_ARMED_MS)
  }

  private abortArmed(): void {
    clearTimeout(this.disarmTimer)
    this.interruptArmed.value = false
    void this.abort()
  }

  async abort(): Promise<void> {
    try {
      await this.host.abort()
    } catch (error) {
      this.host.reportError(error)
    }
  }

  // --- queued prompts and restored drafts -----------------------------------------------

  async removeQueued(messageId: string): Promise<void> {
    if (this.removingQueued.has(messageId)) return
    this.removingQueued.add(messageId)
    try {
      const result = await this.host.api().removeQueued(this.host.sessionId, messageId)
      if (result.removed) this.restore(result.text, result.images)
      else this.host.toast('info', 'composer.toast.alreadySent')
    } catch (error) {
      this.host.reportError(error)
    } finally {
      this.removingQueued.delete(messageId)
    }
  }

  // A prompt taken back (undo, queue): into the box, after a blank line if something is typed.
  restore(text: string, images: readonly NamedImage[]): number {
    this.cursor = restoreInto(this.draft, text, images)
    this.completionController.close()
    return this.cursor
  }

  // --- images and files -----------------------------------------------------------------

  // Pasted or dropped files / file URIs; images attach, unsupported files insert their path at caret.
  async attachFiles(files: readonly File[], uriList = ''): Promise<number> {
    const paths = parseFileUris(uriList)
    if (files.length === 0) {
      for (const path of paths) this.cursor = insertToken(this.draft, path, this.cursor)
      return this.cursor
    }
    for (const [index, file] of files.entries()) {
      const rejection = rejectionOf(file)
      if (rejection === 'size') {
        this.host.toast('warn', 'composer.toast.image.size', { name: file.name })
        continue
      }
      if (rejection === 'type') {
        const path = resolveFilePath(file, index, files.length, paths)
        if (path) this.cursor = insertToken(this.draft, path, this.cursor)
        continue
      }
      try {
        const { image, name } = await readImage(file)
        this.cursor = attachImage(this.draft, image, name, this.cursor)
      } catch (error) {
        this.host.reportError(error)
      }
    }
    return this.cursor
  }
}

export function fileUriToPath(raw: string): string | undefined {
  try {
    const url = new URL(raw)
    if (url.protocol !== 'file:') return undefined
    const decoded = decodeURIComponent(url.pathname)
    if (/^\/[A-Za-z]:\//.test(decoded)) return decoded.slice(1)
    if (url.hostname && url.hostname !== 'localhost') return `//${url.hostname}${decoded}`
    return decoded
  } catch {
    return undefined
  }
}

export function parseFileUris(uriList: string): string[] {
  return uriList
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== '' && !line.startsWith('#'))
    .flatMap((line) => {
      const path = fileUriToPath(line)
      return path === undefined ? [] : [path]
    })
}

function resolveFilePath(file: File, index: number, total: number, paths: readonly string[]): string {
  if (paths.length === total && paths[index]) return paths[index]
  if (file.name) {
    const matched = paths.find(
      (candidate) => candidate.endsWith(`/${file.name}`) || candidate.endsWith(`\\${file.name}`),
    )
    if (matched) return matched
  }
  return file.name
}
