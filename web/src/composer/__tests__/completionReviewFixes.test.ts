import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import CompletionPopup from '../../components/CompletionPopup.vue'
import { CompletionController, FILE_QUERY_DELAY_MS } from '../completion.ts'
import type { Completion, SlashCommand } from '../completion.ts'

const COMMANDS: SlashCommand[] = [
  { name: 'sessions', aliases: ['resume'], title: 'Switch session', requiresArgs: false, custom: false },
]

describe('CompletionPopup Windows paths and duplicate item values', () => {
  it('splits Windows backslash paths for directory muting and renders duplicate values without key collisions', () => {
    const completion: Completion = {
      kind: 'path',
      selected: 0,
      explicitlySelected: false,
      start: 0,
      items: [
        { value: 'src\\components\\App.vue ', label: 'src\\components\\App.vue', description: '' },
        { value: 'C:\\Users\\dev\\folder\\', label: 'C:\\Users\\dev\\folder\\', description: '' },
        { value: 'dup', label: 'first\\dup.ts', description: 'one' },
        { value: 'dup', label: 'second\\dup.ts', description: 'two' },
      ],
    }

    const wrapper = mount(CompletionPopup, {
      props: { completion },
      global: { stubs: { transition: false } },
    })

    const rows = wrapper.findAll('.row')
    expect(rows).toHaveLength(4)

    expect(rows[0]!.get('.path-dir').text()).toBe('src\\components\\')
    expect(rows[0]!.get('.path-name').text()).toBe('App.vue')

    expect(rows[1]!.get('.path-dir').text()).toBe('C:\\Users\\dev\\')
    expect(rows[1]!.get('.path-name').text()).toBe('folder\\')

    expect(rows[2]!.get('.path-dir').text()).toBe('first\\')
    expect(rows[2]!.get('.path-name').text()).toBe('dup.ts')
    expect(rows[3]!.get('.path-dir').text()).toBe('second\\')
    expect(rows[3]!.get('.path-name').text()).toBe('dup.ts')
  })
})

describe('CompletionController /sessions duplicates and !ENV=val shell history', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('disambiguates /sessions candidates that share the same session name by emitting session.id', () => {
    const controller = new CompletionController({
      commands: () => COMMANDS,
      listFiles: async () => [],
      sessions: () => [
        { id: 'ses_live_1', name: 'work', title: 'Current task', cwd: '/repo', live: true },
        { id: 'ses_hist_2', name: 'work', title: 'Archived task', cwd: '/old', live: false },
        { id: 'ses_solo_3', name: 'solo', title: 'Unique name', cwd: '/solo', live: false },
      ],
      reportError: () => {},
    })

    controller.update('/sessions ', 10, false, true)
    expect(controller.completion?.items.map((item) => ({ value: item.value, label: item.label }))).toEqual([
      { value: 'ses_live_1', label: 'work (ses_live_1)' },
      { value: 'ses_hist_2', label: 'work (ses_hist_2)' },
      { value: 'solo', label: 'solo' },
    ])

    // Fuzzy-matching by shared name still surfaces both distinct session IDs.
    controller.update('/sessions work', 14, false, true)
    expect(controller.completion?.items.map((item) => item.value)).toEqual(['ses_live_1', 'ses_hist_2'])
  })

  it('excludes full-line ! history items when typing a command after ENV=val prefixes (start > 0)', async () => {
    const listBinaries = vi.fn(async () => ['git', 'gitk'])
    const controller = new CompletionController({
      commands: () => COMMANDS,
      listFiles: async () => [],
      listBinaries,
      shellHistory: () => ['!git status --short', '!cargo check'],
      reportError: () => {},
    })

    // At column 0 (start === 0, segmentIndex === 0), full-line ! history matches.
    controller.update('git', 3, true, true)
    expect(controller.completion?.items.map((item) => item.value)).toEqual(['git status --short'])

    // After an ENV=val prefix (start === 6 > 0), full-line ! history must be excluded; only binaries match.
    controller.update('FOO=1 git', 9, true, true)
    expect(controller.completion).toBeUndefined()
    await vi.advanceTimersByTimeAsync(FILE_QUERY_DELAY_MS)
    expect(listBinaries).toHaveBeenCalledWith('git', 50)
    expect(controller.completion?.items.map((item) => item.value)).toEqual(['git ', 'gitk '])
    expect(controller.completion?.start).toBe(6)
  })
})
