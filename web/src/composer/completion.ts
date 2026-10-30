// The list that opens above the input while typing `/command` or `@file`
// (the TUI's update_autocomplete).
//
//   "/und|"        -> commands, fuzzy matched on names and aliases
//   "see @src/m|"  -> files from ListFiles, fetched 80 ms after the last key

import { shallowRef } from 'vue'
import { fuzzyScore } from './fuzzy.ts'
import { mentionAt } from './mention.ts'

export interface SlashCommand {
  name: string // canonical, without the slash
  aliases: string[]
  title: string
  takesArgs: boolean
  custom: boolean // from the user's configuration; the built-in ones rank slightly higher
}

export type CompletionKind = 'slash' | 'file'

export interface CompletionItem {
  value: string // what replaces the typed token
  label: string
  description: string
}

export interface Completion {
  kind: CompletionKind
  items: CompletionItem[]
  selected: number
  start: number // where the replaced token starts in the text
}

export interface CompletionDeps {
  commands(): readonly SlashCommand[]
  listFiles(query: string): Promise<string[]>
  reportError(error: unknown): void
}

const MAX_ITEMS = 50
const MAX_FILES = 20
export const FILE_QUERY_DELAY_MS = 80
const BUILTIN_BONUS = 5

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

    const label = best.name === command.name ? `/${command.name}` : `/${best.name} → /${command.name}`
    const bonus = command.custom ? 0 : BUILTIN_BONUS
    scored.push({
      score: best.score + bonus,
      item: { value: `/${command.name} `, label, description: command.title },
    })
  }
  scored.sort((a, b) => b.score - a.score)
  return scored.slice(0, MAX_ITEMS).map((entry) => entry.item)
}

export function fileItems(files: readonly string[]): CompletionItem[] {
  return files.map((file) => ({ value: `@${file} `, label: file, description: '' }))
}

export class CompletionController {
  private readonly deps: CompletionDeps
  private readonly current = shallowRef<Completion | undefined>() // replaced, never edited
  private suppressed = false // Esc closed the list; it stays closed until the text changes
  private fileTimer: ReturnType<typeof setTimeout> | undefined
  private filesInFlight = false
  private wantedQuery: string | undefined // asked for while a request was running
  private mention: { start: number; query: string } | undefined

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

  // Call after every change of the text or the cursor. `edited`: the text changed (not only the cursor).
  update(text: string, cursor: number, shell: boolean, edited: boolean): void {
    if (edited) this.suppressed = false
    clearTimeout(this.fileTimer)
    this.mention = undefined
    if (shell || this.suppressed) return this.close()

    const query = text.slice(1)
    const typingCommand = text.startsWith('/') && !/\s/.test(query) && cursor === text.length
    if (typingCommand) return this.showCommands(query)

    this.mention = mentionAt(text, cursor)
    if (!this.mention) return this.close()
    if (this.current.value?.kind === 'slash') this.close()
    const { query: fileQuery } = this.mention
    this.fileTimer = setTimeout(() => this.requestFiles(fileQuery), FILE_QUERY_DELAY_MS)
  }

  move(delta: number): void {
    const completion = this.current.value
    if (!completion || completion.items.length === 0) return
    const count = completion.items.length
    const selected = (((completion.selected + delta) % count) + count) % count
    this.current.value = { ...completion, selected }
  }

  // Esc: close the list, and keep it closed while the text stays the same.
  dismiss(): void {
    this.suppressed = true
    clearTimeout(this.fileTimer)
    this.close()
  }

  close(): void {
    this.current.value = undefined
  }

  private showCommands(query: string): void {
    const items = slashItems(this.deps.commands(), query)
    if (items.length === 0) return this.close()

    const previous = this.current.value
    const keep = previous?.kind === 'slash' && query === '' ? previous.selected : 0
    this.current.value = { kind: 'slash', items, selected: Math.min(keep, items.length - 1), start: 0 }
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

  // A newer query arrived while this one ran: ask for it now.
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
      start: mention.start,
    }
  }
}
