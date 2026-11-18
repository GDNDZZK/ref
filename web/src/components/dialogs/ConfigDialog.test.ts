import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../../i18n/index.ts'
import type { ConfigFilesInfo } from '../../proto/generated'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import ConfigDialog from './ConfigDialog.vue'

function makeInfo(overrides: Partial<ConfigFilesInfo> = {}): ConfigFilesInfo {
  const raw =
    '{\n  "$name": "Work",\n  "model": "anthropic/claude-sonnet-4-5",\n  "customKey": { "keep": 1 },\n  "provider": {\n    "anthropic": {\n      "options": {\n        "apiKey": "{env:ANTHROPIC_API_KEY}"\n      }\n    }\n  },\n  "agent": {\n    "build": {\n      "color": "blue"\n    }\n  }\n}\n'
  return {
    config_dir: '/home/user/.config/ref',
    files: [
      {
        file: 'ref.json',
        active: true,
        exists: true,
        label: 'Work',
        summary: '1 provider · anthropic/claude-sonnet-4-5',
        modified_at: 1000,
        size: raw.length,
      },
      {
        file: 'ref.json.1.bak',
        active: false,
        exists: true,
        label: 'Personal',
        summary: '1 provider · openai/gpt-5',
        modified_at: 900,
        size: 120,
      },
    ],
    selected_file: 'ref.json',
    raw_text: raw,
    parsed: JSON.parse(raw),
    env_status: { ANTHROPIC_API_KEY: true },
    has_jsonc_override: true,
    env_config_override: '/tmp/override.json',
    project_overrides: ['/work/.ref/ref.json'],
    warnings: ['sample warning'],
    ...overrides,
  }
}

describe('ConfigDialog', () => {
  let pinia = createPinia()

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
  })

  afterEach(() => {
    document.body.innerHTML = ''
    vi.unstubAllGlobals()
  })

  async function setup(initial = makeInfo(), section?: 'providers' | 'agents') {
    const connection = useConnectionStore()
    const dialogs = useDialogsStore()
    const workspace = useWorkspaceStore()
    workspace.cwd = '/work'

    const getConfigFiles = vi.spyOn(connection, 'getConfigFiles').mockResolvedValue(initial)
    const saveConfigFile = vi
      .spyOn(connection, 'saveConfigFile')
      .mockImplementation(async (file, content) =>
        makeInfo({ selected_file: file, raw_text: content, parsed: JSON.parse(content) }),
      )
    const createConfigFile = vi
      .spyOn(connection, 'createConfigFile')
      .mockResolvedValue(makeInfo({ raw_text: '{\n  "$name": "Lab"\n}\n', parsed: { $name: 'Lab' } }))
    const selectConfigFile = vi.spyOn(connection, 'selectConfigFile').mockResolvedValue(initial)
    const deleteConfigFile = vi.spyOn(connection, 'deleteConfigFile').mockResolvedValue(initial)
    const reloadConfig = vi.spyOn(connection, 'reloadConfig').mockResolvedValue({
      config: {
        agents: [],
        default_agent: 'build',
        models: [],
        commands: [],
        mcp: [],
        tui: null,
        warnings: [],
      },
      warnings: [],
    })

    dialogs.open({ kind: 'config', section })
    const wrapper = mount(ConfigDialog, {
      props: { section },
      attachTo: document.body,
      global: { plugins: [pinia, createAppI18n(['en'])] },
    })
    await flushPromises()
    return {
      wrapper,
      dialogs,
      getConfigFiles,
      saveConfigFile,
      createConfigFile,
      selectConfigFile,
      deleteConfigFile,
      reloadConfig,
    }
  }

  it('loads profiles, displays override banners, and preserves untouched keys across visual edits and save', async () => {
    const { wrapper, getConfigFiles, saveConfigFile } = await setup()
    expect(getConfigFiles).toHaveBeenCalledWith(undefined, '/work')
    expect(wrapper.text()).toContain('ref.jsonc also exists')
    expect(wrapper.text()).toContain('/tmp/override.json')
    expect(wrapper.text()).toContain('/work/.ref/ref.json')
    expect(wrapper.text()).toContain('sample warning')
    expect(wrapper.get('.file-path').text()).toBe('/home/user/.config/ref/ref.json')

    // Edit profile label ($name) in General section
    const nameInput = wrapper.get('input[placeholder="e.g. Work, Local"]')
    await nameInput.setValue('Studio')

    // Navigate through all 6 sections
    const navButtons = wrapper.findAll('.nav-item')
    expect(navButtons).toHaveLength(6)
    await navButtons[1]!.trigger('click') // Providers
    expect(wrapper.text()).toContain('anthropic')

    await navButtons[2]!.trigger('click') // Agents
    expect(wrapper.text()).toContain('Customized')

    await navButtons[3]!.trigger('click') // Permissions & Delays
    expect(wrapper.text()).toContain('Global tool permissions')

    await navButtons[4]!.trigger('click') // MCP Servers
    await wrapper
      .findAll('button')
      .find((b) => b.text().includes('Add local server'))!
      .trigger('click')
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'Create')!
      .trigger('click')
    const pasteInput = wrapper.get('input[placeholder*="npx -y"]')
    await pasteInput.setValue('npx -y "@mcp/fs" /work')
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'Split into argv')!
      .trigger('click')

    await navButtons[5]!.trigger('click') // Commands & Skills
    expect(wrapper.text()).toContain('Custom slash commands')

    // Save via Ctrl+S
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true }))
    await flushPromises()
    expect(saveConfigFile).toHaveBeenCalledTimes(1)
    const savedJson = JSON.parse(saveConfigFile.mock.calls[0]![1])
    expect(savedJson.$name).toBe('Studio')
    expect(savedJson.customKey).toEqual({ keep: 1 })
    expect(savedJson.provider.anthropic.options.apiKey).toBe('{env:ANTHROPIC_API_KEY}')
    expect(savedJson['mcp']['local-mcp'].command).toEqual(['npx', '-y', '@mcp/fs', '/work'])
    wrapper.unmount()
  })

  it('parses JSONC when switching JSON -> Visual and blocks invalid JSON without losing edits', async () => {
    const { wrapper } = await setup()
    const radios = wrapper.findAll('.topbar-right [role="radio"]')
    await radios[1]!.trigger('click') // JSON mode

    const area = wrapper.get('.json-pane textarea')
    await area.setValue('{ invalid json ')
    await radios[0]!.trigger('click') // attempt Visual mode
    expect(wrapper.text()).toContain('JSON syntax error')
    expect((wrapper.get('.json-pane textarea').element as HTMLTextAreaElement).value).toBe('{ invalid json ')

    // Valid JSONC with comment + trailing comma switches cleanly to Visual mode
    await area.setValue('{\n  // comment\n  "$name": "FromJsonc",\n}\n')
    await radios[0]!.trigger('click')
    expect(wrapper.find('.json-pane').exists()).toBe(false)
    expect((wrapper.get('input[placeholder="e.g. Work, Local"]').element as HTMLInputElement).value).toBe(
      'FromJsonc',
    )
    wrapper.unmount()
  })

  it('guards Esc, scrim click, and profile switching when dirty until Discard is confirmed', async () => {
    const { wrapper, dialogs, selectConfigFile, createConfigFile, deleteConfigFile } = await setup()
    const nameInput = wrapper.get('input[placeholder="e.g. Work, Local"]')
    await nameInput.setValue('UnsavedName')

    // Esc and scrim click are intercepted while dirty
    dialogs.requestClose('escape')
    await flushPromises()
    expect(dialogs.current?.kind).toBe('config')
    expect(wrapper.text()).toContain('Unsaved changes')

    // Cancel keeps dialog open and dirty
    await wrapper
      .findAll('.prompt-bar button')
      .find((b) => b.text() === 'Cancel')!
      .trigger('click')
    expect(wrapper.find('.prompt-bar').exists()).toBe(false)

    // Profile switch while dirty triggers unsaved prompt; Discard proceeds with selectConfigFile
    const select = wrapper.get('select.profile-select')
    ;(select.element as HTMLSelectElement).value = 'ref.json.1.bak'
    await select.trigger('change')
    expect(selectConfigFile).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Unsaved changes')

    await wrapper
      .findAll('.prompt-bar button')
      .find((b) => b.text() === 'Discard')!
      .trigger('click')
    await flushPromises()
    expect(selectConfigFile).toHaveBeenCalledWith('ref.json.1.bak', '/work')

    // + New profile and Delete profile
    await wrapper.find('button[title="New"]').trigger('click')
    await wrapper.get('.inline-input').setValue('Lab')
    await wrapper
      .findAll('.prompt-bar button')
      .find((b) => b.text() === 'Create')!
      .trigger('click')
    await flushPromises()
    expect(createConfigFile).toHaveBeenCalledWith({ label: 'Lab', cloneCurrent: true, cwd: '/work' })

    await wrapper.find('button[title="Delete"]').trigger('click')
    expect(wrapper.text()).toContain('newest backup will become active')
    await wrapper
      .findAll('.prompt-bar button')
      .find((b) => b.text() === 'Delete')!
      .trigger('click')
    await flushPromises()
    expect(deleteConfigFile).toHaveBeenCalledWith('ref.json', '/work')
    wrapper.unmount()
  })
})
