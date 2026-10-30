// The input box of one session: draft, history, completion list, sending and taking back.
// A ChatSession owns one, so the draft survives switching to another session and back.
//
//   keys / paste ---> SessionComposer ---> Prompt | Shell | slash command | ExpandCommand
//   Undo / RemoveQueued ---> restore() ---> back into the draft

import { reactive, ref } from 'vue'
import type { RefApi } from '../net/api.ts'
import type { NamedImage } from '../proto/generated'
import { onDraftRestored } from '../stores/draftBus.ts'
import { CompletionController } from './completion.ts'
import type { SlashCommand } from './completion.ts'
import {
  attachImage,
  clearDraft,
  dropOrphanAttachments,
  emptyDraft,
  restoreInto,
  submissionOf,
} from './draft.ts'
import type { Draft } from './draft.ts'
import { HistoryBrowser } from './history.ts'
import type { PromptHistory } from './history.ts'
import { readImage, rejectionOf } from './images.ts'

export type ComposerApi = Pick<RefApi, 'prompt' | 'shell' | 'removeQueued' | 'listFiles' | 'expandCommand'>

export type ToastLevel = 'info' | 'warn' | 'error'

export interface ComposerHost {
  sessionId: string
  api(): ComposerApi
  history: { history: PromptHistory; push(text: string): void }
  cwd(): string
  isBusy(): boolean
  hasDelayedTool(): boolean // the last tool call waits out its delay: one Esc cancels it
  slashCommands(): readonly SlashCommand[]
  runSlash(line: string): Promise<void> // a built-in command, e.g. "/rename Work"
  abort(): Promise<void> // stops the running turn
  toast(level: ToastLevel, messageKey: string, params?: Record<string, string>): void
  reportError(error: unknown): void
}

export const INTERRUPT_ARMED_MS = 1500
const SHELL_PREFIX = '!'
const FILE_LIMIT = 20
const MIN_CHARS_TO_KEEP = 20 // a cleared text this long is kept in the history

export class SessionComposer {
  readonly draft: Draft = reactive(emptyDraft())
  readonly interruptArmed = ref(false) // Esc was pressed once; a second one within INTERRUPT_ARMED_MS aborts
  readonly removingQueued = reactive(new Set<string>()) // message ids being taken back

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
      commands: () => host.slashCommands(),
      listFiles: (query) => host.api().listFiles(host.cwd(), query, FILE_LIMIT),
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
    this.completionController.close()
  }

  leaveShell(): void {
    this.draft.shell = false
  }

  // --- history --------------------------------------------------------------------------

  // ↑ with the caret at the start (or in an empty box). Returns the new caret when it acted.
  historyPrev(): number | undefined {
    const entry = this.browser.prev(this.draft.shell ? SHELL_PREFIX + this.draft.text : this.draft.text)
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

  // Tab inserts the choice; Enter (`run`) also runs a command that needs no arguments.
  // Returns the new caret when the text changed.
  acceptCompletion(run: boolean): number | undefined {
    const completion = this.completionController.completion
    const item = this.completionController.selectedItem
    if (!completion || !item) return undefined
    this.completionController.close()

    if (completion.kind === 'file') {
      const text = this.draft.text
      const replaced = text.slice(0, completion.start) + item.value + text.slice(this.cursor)
      this.cursor = completion.start + item.value.length
      this.draft.text = replaced
      this.completionController.update(replaced, this.cursor, this.draft.shell, true)
      return this.cursor
    }
    return this.acceptCommand(item.value, run)
  }

  private acceptCommand(value: string, run: boolean): number | undefined {
    const name = value.trim().slice(1)
    const commands = this.host.slashCommands()
    const takesArgs = commands.find((command) => command.name === name)?.takesArgs ?? true

    const typed = this.draft.text.trim().replace(/^\/+/, '')
    const typedCommand = commands.find((c) => c.name === typed || c.aliases.includes(typed))
    const typedFully = (typedCommand?.name ?? typed) === name
    if (run && takesArgs && typedFully) {
      this.draft.text = `/${name}`
      void this.submit()
      return undefined
    }
    const command = commands.find((candidate) => candidate.name === name)
    if (run && !takesArgs && command) {
      this.host.history.push(`/${name}`)
      clearDraft(this.draft)
      void this.runCommand(command, `/${name}`)
      return undefined
    }
    return this.setText(value)
  }

  // The command a line such as "/rename Work" names, if there is one.
  private commandIn(line: string): SlashCommand | undefined {
    if (!line.startsWith('/')) return undefined
    const typed = (line.slice(1).split(/\s+/)[0] ?? '').toLowerCase()
    return this.host
      .slashCommands()
      .find((command) => command.name === typed || command.aliases.includes(typed))
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
    this.browser.reset()

    if (draft.shell) return this.submitShell(trimmed)
    const command = this.commandIn(trimmed)
    if (!command) return this.submitPrompt()

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

  // --- images ---------------------------------------------------------------------------

  // Pasted or dropped files; non-images are skipped with a message. Returns the caret after the tokens.
  async attachFiles(files: readonly File[]): Promise<number> {
    for (const file of files) {
      const rejection = rejectionOf(file)
      if (rejection) {
        this.host.toast('warn', `composer.toast.image.${rejection}`, { name: file.name })
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
