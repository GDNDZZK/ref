import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { LOCALES, createAppI18n } from '../../../i18n/index.ts'
import type { JsonObject } from '../../../config/jsonPatch.ts'
import { configuredVariantOptions } from '../../../config/sectionHelpers.ts'
import type { ConfigFilesInfo, ModelInfo } from '../../../proto/generated'
import { useConnectionStore } from '../../../stores/connection.ts'
import { useDialogsStore } from '../../../stores/dialogs.ts'
import { useWorkspaceStore } from '../../../stores/workspace.ts'
import ConfigDialog from '../../dialogs/ConfigDialog.vue'
import { PermissionRulesEditor, StringListEditor } from '../index.ts'
import AgentsSection from '../sections/AgentsSection.vue'
import CommandsSection from '../sections/CommandsSection.vue'
import GeneralSection from '../sections/GeneralSection.vue'
import McpSection from '../sections/McpSection.vue'
import ProvidersSection from '../sections/ProvidersSection.vue'

function i18nGlobal(locale = 'en') {
  return { plugins: [createAppI18n([locale])] }
}

function makeConfigInfo(parsed: JsonObject): ConfigFilesInfo {
  const raw = `${JSON.stringify(parsed, null, 2)}\n`
  return {
    config_dir: '/home/user/.config/ref',
    files: [
      {
        file: 'ref.json',
        active: true,
        exists: true,
        label: typeof parsed.$name === 'string' ? parsed.$name : 'Work',
        summary: '',
        modified_at: 1000,
        size: raw.length,
      },
      {
        file: 'ref.json.1.bak',
        active: false,
        exists: true,
        label: 'Backup',
        summary: '',
        modified_at: 900,
        size: 32,
      },
    ],
    selected_file: 'ref.json',
    raw_text: raw,
    parsed: JSON.parse(raw),
    env_status: { ANTHROPIC_API_KEY: true },
    has_jsonc_override: false,
    env_config_override: null,
    project_overrides: [],
    warnings: [],
  }
}

describe('Prose config fields preserve trailing spaces during @input and trim on @blur', () => {
  it('preserves trailing spaces while typing $name in GeneralSection and trims on blur', async () => {
    let doc: JsonObject = { $name: 'Work' }
    const wrapper = mount(GeneralSection, {
      props: {
        doc,
        'onUpdate:doc': (next: JsonObject) => {
          doc = next
          void wrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })

    const input = wrapper.get('input[placeholder="e.g. Work, Local"]')
    await input.setValue('Work ')
    expect((input.element as HTMLInputElement).value).toBe('Work ')
    expect(doc.$name).toBe('Work ')

    await input.setValue('Work Profile ')
    expect((input.element as HTMLInputElement).value).toBe('Work Profile ')
    expect(doc.$name).toBe('Work Profile ')

    await input.trigger('blur')
    expect(doc.$name).toBe('Work Profile')
  })

  it('preserves trailing spaces while typing provider name, model name, agent description, and command description', async () => {
    let doc: JsonObject = {
      provider: { openai: { models: { 'gpt-5': {} } } },
      agent: { reviewer: { description: 'Initial' } },
      command: { review: { description: 'Initial', template: '$ARGUMENTS' } },
    }

    const providersWrapper = mount(ProvidersSection, {
      props: {
        doc,
        'onUpdate:doc': (next: JsonObject) => {
          doc = next
          void providersWrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })
    await providersWrapper.get('.expand-btn').trigger('click')
    const providerNameInput = providersWrapper.get('input[placeholder="openai"]')
    await providerNameInput.setValue('OpenAI ')
    expect((providerNameInput.element as HTMLInputElement).value).toBe('OpenAI ')
    expect(((doc.provider as JsonObject).openai as JsonObject).name).toBe('OpenAI ')
    await providerNameInput.trigger('blur')
    expect(((doc.provider as JsonObject).openai as JsonObject).name).toBe('OpenAI')

    await providersWrapper.get('.model-card .expand-btn').trigger('click')
    const modelNameInput = providersWrapper.get('.model-card input[placeholder="gpt-5"]')
    await modelNameInput.setValue('GPT Five ')
    expect((modelNameInput.element as HTMLInputElement).value).toBe('GPT Five ')
    expect((((doc.provider as JsonObject).openai as JsonObject).models as JsonObject)['gpt-5']).toEqual({
      name: 'GPT Five ',
    })
    await modelNameInput.trigger('blur')
    expect((((doc.provider as JsonObject).openai as JsonObject).models as JsonObject)['gpt-5']).toEqual({
      name: 'GPT Five',
    })

    const agentsWrapper = mount(AgentsSection, {
      props: {
        doc,
        'onUpdate:doc': (next: JsonObject) => {
          doc = next
          void agentsWrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })
    // Expand build (index 0) and custom reviewer (last card).
    const agentCards = agentsWrapper.findAll('.card')
    await agentCards[0]!.get('.expand-btn').trigger('click')
    const buildDescInput = agentCards[0]!.get('input.text-input')
    await buildDescInput.setValue('Primary builder ')
    expect((buildDescInput.element as HTMLInputElement).value).toBe('Primary builder ')
    expect(((doc.agent as JsonObject).build as JsonObject).description).toBe('Primary builder ')
    await buildDescInput.trigger('blur')
    expect(((doc.agent as JsonObject).build as JsonObject).description).toBe('Primary builder')

    const commandsWrapper = mount(CommandsSection, {
      props: {
        doc,
        'onUpdate:doc': (next: JsonObject) => {
          doc = next
          void commandsWrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })
    await commandsWrapper.get('.expand-btn').trigger('click')
    const cmdInputs = commandsWrapper.findAll('.card-body input.text-input')
    const cmdDescInput = cmdInputs[1]!
    await cmdDescInput.setValue('Run code ')
    expect((cmdDescInput.element as HTMLInputElement).value).toBe('Run code ')
    expect(((doc.command as JsonObject).review as JsonObject).description).toBe('Run code ')
    await cmdDescInput.trigger('blur')
    expect(((doc.command as JsonObject).review as JsonObject).description).toBe('Run code')
  })
})

describe('Opencode alias keys (thinking, modalities.input, spawn)', () => {
  it('clears thinking when toggling or resetting model reasoning, and removes image modality when toggling attachment off', async () => {
    let doc: JsonObject = {
      provider: {
        anthropic: {
          models: {
            sonnet: {
              thinking: true,
              modalities: { input: ['text', 'image'], output: ['text'] },
            },
            visionOnly: {
              modalities: { input: ['image'] },
            },
          },
        },
      },
    }

    const wrapper = mount(ProvidersSection, {
      props: {
        doc,
        'onUpdate:doc': (next: JsonObject) => {
          doc = next
          void wrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })

    await wrapper.get('.expand-btn').trigger('click')
    const modelCards = wrapper.findAll('.model-card')
    await modelCards[0]!.get('.expand-btn').trigger('click')

    const attachmentSwitch = modelCards[0]!.get('button[aria-label="Image input (attachment)"]')
    const reasoningSwitch = modelCards[0]!.get('button[aria-label="Reasoning / thinking"]')
    expect(attachmentSwitch.attributes('aria-checked')).toBe('true')
    expect(reasoningSwitch.attributes('aria-checked')).toBe('true')

    // Toggle reasoning OFF -> removes `thinking` and writes `reasoning: false`.
    await reasoningSwitch.trigger('click')
    const sonnetAfterReasoning = (((doc.provider as JsonObject).anthropic as JsonObject).models as JsonObject)
      .sonnet as JsonObject
    expect(sonnetAfterReasoning.thinking).toBeUndefined()
    expect(sonnetAfterReasoning.reasoning).toBe(false)
    expect(modelCards[0]!.get('button[aria-label="Reasoning / thinking"]').attributes('aria-checked')).toBe(
      'false',
    )

    // Toggle attachment OFF -> writes `attachment: false`, removes `'image'`, preserves `'text'` & `output`.
    await attachmentSwitch.trigger('click')
    const sonnetAfterAttachment = (
      ((doc.provider as JsonObject).anthropic as JsonObject).models as JsonObject
    ).sonnet as JsonObject
    expect(sonnetAfterAttachment.attachment).toBe(false)
    expect(sonnetAfterAttachment.modalities).toEqual({ input: ['text'], output: ['text'] })
    expect(
      modelCards[0]!.get('button[aria-label="Image input (attachment)"]').attributes('aria-checked'),
    ).toBe('false')

    // When `'image'` was the sole modality, toggling attachment OFF prunes empty `modalities.input` and `modalities`.
    await modelCards[1]!.get('.expand-btn').trigger('click')
    await modelCards[1]!.get('button[aria-label="Image input (attachment)"]').trigger('click')
    const visionAfter = (((doc.provider as JsonObject).anthropic as JsonObject).models as JsonObject)
      .visionOnly as JsonObject
    expect(visionAfter).toEqual({ attachment: false })
  })

  it('reads spawn alias, clears spawn on toggle or reset, and exposes reset controls on custom agent cards', async () => {
    let doc: JsonObject = {
      agent: {
        plan: { spawn: true },
        helper: { description: 'Custom helper', spawn: true, creatable: false, hidden: true },
      },
    }

    const wrapper = mount(AgentsSection, {
      props: {
        doc,
        'onUpdate:doc': (next: JsonObject) => {
          doc = next
          void wrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })

    const cards = wrapper.findAll('.card')
    const planCard = cards[1]! // build=0, plan=1
    const helperCard = cards[cards.length - 1]!

    await planCard.get('.expand-btn').trigger('click')
    const planSpawnSwitch = planCard.get('button[aria-label="Can spawn sub-sessions"]')
    expect(planSpawnSwitch.attributes('aria-checked')).toBe('true')
    await planSpawnSwitch.trigger('click')
    expect((doc.agent as JsonObject).plan).toEqual({ can_spawn: false })
    expect(planCard.get('button[aria-label="Can spawn sub-sessions"]').attributes('aria-checked')).toBe(
      'false',
    )

    // Custom agent card supports spawn alias + `:can-reset` on creatable, can_spawn, and hidden.
    await helperCard.get('.expand-btn').trigger('click')
    const helperSpawnSwitch = helperCard.get('button[aria-label="Can spawn sub-sessions"]')
    expect(helperSpawnSwitch.attributes('aria-checked')).toBe('true')

    const resetButtons = helperCard.findAll('.form-row .text-button')
    expect(resetButtons).toHaveLength(3) // creatable, can_spawn (via spawn), hidden
    await resetButtons[1]!.trigger('click') // reset can_spawn/spawn
    expect((doc.agent as JsonObject).helper).toEqual({
      description: 'Custom helper',
      creatable: false,
      hidden: true,
    })
  })
})

describe('StringListEditor stable row identity when clearing items', () => {
  it('keeps row 0 focused and empty when cleared instead of shifting row 1 into row 0', async () => {
    let filteredItems = ['docs/A.md', 'docs/B.md']
    const wrapper = mount(StringListEditor, {
      props: {
        modelValue: filteredItems,
        monospace: true,
        'onUpdate:modelValue': (next: string[]) => {
          filteredItems = next.filter((entry) => entry.trim() !== '')
          void wrapper.setProps({ modelValue: filteredItems })
        },
      },
      global: i18nGlobal(),
    })

    const initialInputs = wrapper.findAll('.item-row input')
    expect(initialInputs).toHaveLength(2)
    const firstInputEl = initialInputs[0]!.element as HTMLInputElement

    // Select-all + Backspace on row 0.
    await initialInputs[0]!.setValue('')
    await wrapper.vm.$nextTick()

    const afterClearInputs = wrapper.findAll('.item-row input')
    expect(afterClearInputs).toHaveLength(2)
    expect(afterClearInputs[0]!.element).toBe(firstInputEl)
    expect((afterClearInputs[0]!.element as HTMLInputElement).value).toBe('')
    expect((afterClearInputs[1]!.element as HTMLInputElement).value).toBe('docs/B.md')
    expect(filteredItems).toEqual(['docs/B.md'])

    // Typing replacement text into row 0 restores its position before row 1.
    await afterClearInputs[0]!.setValue('docs/C.md')
    await wrapper.vm.$nextTick()
    expect(filteredItems).toEqual(['docs/C.md', 'docs/B.md'])
  })
})

describe('Visual options JSON editors, inline renames, and ConfigDialog validation / Discard', () => {
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

  async function mountConfigDialog(
    parsed: JsonObject,
    section?: 'providers' | 'agents' | 'mcp' | 'commands',
  ) {
    const connection = useConnectionStore()
    const dialogs = useDialogsStore()
    const workspace = useWorkspaceStore()
    workspace.cwd = '/work'

    const initial = makeConfigInfo(parsed)
    vi.spyOn(connection, 'getConfigFiles').mockResolvedValue(initial)
    const saveConfigFile = vi
      .spyOn(connection, 'saveConfigFile')
      .mockImplementation(async (_file, content) => makeConfigInfo(JSON.parse(content) as JsonObject))
    const selectConfigFile = vi
      .spyOn(connection, 'selectConfigFile')
      .mockResolvedValue(makeConfigInfo(parsed))

    dialogs.open({ kind: 'config', section })
    const wrapper = mount(ConfigDialog, {
      props: { section },
      attachTo: document.body,
      global: { plugins: [pinia, createAppI18n(['en'])] },
    })
    await flushPromises()
    return { wrapper, dialogs, saveConfigFile, selectConfigFile }
  }

  it('edits provider, model, and agent options JSON losslessly while preserving apiKey/baseURL/headers, blocks Save on invalid JSON, and resets on Discard', async () => {
    const { wrapper, dialogs, saveConfigFile } = await mountConfigDialog(
      {
        $name: 'Work',
        provider: {
          anthropic: {
            options: {
              apiKey: '{env:ANTHROPIC_API_KEY}',
              baseURL: 'https://api.anthropic.com/v1',
              setCacheKey: true,
              chunkTimeout: 120000,
              headers: { 'X-Org': 'acme' },
              timeout: 30000,
            },
            models: {
              'claude-sonnet-4-5': {},
            },
          },
        },
        agent: {
          reviewer: { description: 'Reviewer' },
        },
      },
      'providers',
    )

    // Expand anthropic provider and claude-sonnet-4-5 model.
    await wrapper.get('.card .expand-btn').trigger('click')
    await wrapper.get('.model-card .expand-btn').trigger('click')

    const providerOptionsArea = wrapper.get('textarea[aria-label="Extra SDK options (JSON object)"]')
    expect((providerOptionsArea.element as HTMLTextAreaElement).value).toBe('{\n  "timeout": 30000\n}')

    // Invalid JSON in provider extra options marks ConfigDialog dirty, blocks Esc, and disables Save even with other valid edits!
    await providerOptionsArea.setValue('{ invalid')
    expect(wrapper.text()).toContain('Must be a valid JSON object')

    dialogs.requestClose('escape')
    await flushPromises()
    expect(dialogs.current?.kind).toBe('config')
    expect(wrapper.text()).toContain('Unsaved changes')
    await wrapper
      .findAll('.prompt-bar button')
      .find((btn) => btn.text() === 'Cancel')!
      .trigger('click')

    // Edit model options validly while provider extra options is still invalid -> Save must stay blocked!
    const modelOptionsArea = wrapper.get('textarea[aria-label="Model options (JSON object)"]')
    await modelOptionsArea.setValue('{ "reasoningEffort": "high" }')

    const saveBtn = wrapper.findAll('.footer-actions button').find((btn) => btn.text() === 'Save')!
    expect(saveBtn.attributes('disabled')).toBeDefined()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true }))
    await flushPromises()
    expect(saveConfigFile).not.toHaveBeenCalled()

    // Discard resets both doc and invalid sub-editor drafts/errors back to saved state!
    const discardBtn = wrapper.findAll('.footer-actions button').find((btn) => btn.text() === 'Discard')!
    await discardBtn.trigger('click')
    await flushPromises()
    expect(wrapper.text()).not.toContain('Must be a valid JSON object')
    expect(
      (wrapper.get('textarea[aria-label="Extra SDK options (JSON object)"]').element as HTMLTextAreaElement)
        .value,
    ).toBe('{\n  "timeout": 30000\n}')
    expect(
      (wrapper.get('textarea[aria-label="Model options (JSON object)"]').element as HTMLTextAreaElement)
        .value,
    ).toBe('')

    // Now write valid JSON to provider extra options, model options, and agent options and Save!
    await wrapper
      .get('textarea[aria-label="Extra SDK options (JSON object)"]')
      .setValue('{ "timeout": 60000, "extraBody": { "topK": 32 } }')
    await wrapper
      .get('textarea[aria-label="Model options (JSON object)"]')
      .setValue('{ "reasoningEffort": "high" }')

    // Switch to Agents section and edit both built-in `build` and custom `reviewer` options.
    await wrapper.findAll('.nav-item')[2]!.trigger('click')
    const agentCards = wrapper.findAll('.card')
    await agentCards[0]!.get('.expand-btn').trigger('click') // build
    await agentCards[agentCards.length - 1]!.get('.expand-btn').trigger('click') // reviewer

    const agentOptionAreas = wrapper.findAll('textarea[aria-label="Model options (JSON object)"]')
    expect(agentOptionAreas).toHaveLength(2)
    await agentOptionAreas[0]!.setValue('{ "maxTokens": 8192 }')
    await agentOptionAreas[1]!.setValue('{ "maxTokens": 4096 }')

    await wrapper
      .findAll('.footer-actions button')
      .find((btn) => btn.text() === 'Save')!
      .trigger('click')
    await flushPromises()

    expect(saveConfigFile).toHaveBeenCalledTimes(1)
    const saved = JSON.parse(saveConfigFile.mock.calls[0]![1])
    expect(saved.provider.anthropic.options).toEqual({
      apiKey: '{env:ANTHROPIC_API_KEY}',
      baseURL: 'https://api.anthropic.com/v1',
      setCacheKey: true,
      chunkTimeout: 120000,
      headers: { 'X-Org': 'acme' },
      timeout: 60000,
      extraBody: { topK: 32 },
    })
    expect(saved.provider.anthropic.models['claude-sonnet-4-5'].options).toEqual({
      reasoningEffort: 'high',
    })
    expect(saved.agent.build.options).toEqual({ maxTokens: 8192 })
    expect(saved.agent.reviewer.options).toEqual({ maxTokens: 4096 })
    wrapper.unmount()
  })

  it('commits inline renames on blur, blocks and warns on collisions, and resets renameDrafts on Discard', async () => {
    const { wrapper } = await mountConfigDialog(
      {
        agent: {
          alpha: { description: 'Alpha agent' },
          beta: { description: 'Beta agent' },
        },
        mcp: {
          first: { type: 'local', command: ['npx'] },
          second: { type: 'remote', url: 'https://example.com/mcp' },
        },
      },
      'agents',
    )

    // Expand custom agent `alpha`.
    const customCards = wrapper.findAll('.card').slice(5)
    expect(customCards).toHaveLength(2)
    await customCards[0]!.get('.expand-btn').trigger('click')

    const renameRow = customCards[0]!.get('.rename-row')
    const renameInput = renameRow.get('input')
    const renameBtn = renameRow.get('button')

    // Disabled when unchanged.
    expect(renameBtn.attributes('disabled')).toBeDefined()

    // Colliding with existing custom agent `beta` OR built-in `build` disables Rename and shows error alert.
    await renameInput.setValue('beta')
    expect(renameBtn.attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('An entry with this name already exists')

    await renameInput.setValue('build')
    expect(renameBtn.attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('An entry with this name already exists')

    // Discard resets colliding renameDraft back to `alpha`!
    const discardBtn = wrapper.findAll('.footer-actions button').find((btn) => btn.text() === 'Discard')!
    await discardBtn.trigger('click')
    await flushPromises()
    expect((customCards[0]!.get('.rename-row input').element as HTMLInputElement).value).toBe('alpha')
    expect(wrapper.text()).not.toContain('An entry with this name already exists')

    // Valid non-colliding rename commits automatically on @blur!
    const refreshedInput = wrapper.findAll('.card').slice(5)[0]!.get('.rename-row input')
    await refreshedInput.setValue('gamma')
    await refreshedInput.trigger('blur')
    await flushPromises()
    expect(
      wrapper
        .findAll('.card')
        .slice(5)
        .map((card) => card.get('.card-id').text()),
    ).toEqual(['gamma', 'beta'])

    // Verify McpSection inline rename collision guard too.
    const mcpWrapper = mount(McpSection, {
      props: {
        doc: {
          mcp: {
            first: { type: 'local', command: ['npx'] },
            second: { type: 'remote', url: 'https://example.com' },
          },
        },
      },
      global: i18nGlobal(),
    })
    await mcpWrapper.findAll('.card')[0]!.get('.expand-btn').trigger('click')
    await mcpWrapper.get('.rename-row input').setValue('second')
    expect(mcpWrapper.get('.rename-row button').attributes('disabled')).toBeDefined()
    expect(mcpWrapper.text()).toContain('An entry with this name already exists')

    // Verify ProvidersSection and CommandsSection inline rename @blur commit + collision guard too.
    let providerDoc: JsonObject = { provider: { openai: {}, anthropic: {} } }
    const provWrapper = mount(ProvidersSection, {
      props: {
        doc: providerDoc,
        'onUpdate:doc': (next: JsonObject) => {
          providerDoc = next
          void provWrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })
    await provWrapper.findAll('.card')[0]!.get('.expand-btn').trigger('click')
    await provWrapper.get('.rename-row input').setValue('anthropic')
    expect(provWrapper.get('.rename-row button').attributes('disabled')).toBeDefined()
    expect(provWrapper.text()).toContain('An entry with this name already exists')
    await provWrapper.get('.rename-row input').setValue('openai-gateway')
    await provWrapper.get('.rename-row input').trigger('blur')
    expect(Object.keys((providerDoc.provider as JsonObject) ?? {})).toEqual(['openai-gateway', 'anthropic'])

    let cmdDoc: JsonObject = {
      command: { review: { template: '$1' }, deploy: { template: '$2' } },
    }
    const cmdWrapper = mount(CommandsSection, {
      props: {
        doc: cmdDoc,
        'onUpdate:doc': (next: JsonObject) => {
          cmdDoc = next
          void cmdWrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })
    await cmdWrapper.findAll('.card')[0]!.get('.expand-btn').trigger('click')
    await cmdWrapper.get('.rename-row input').setValue('/deploy')
    expect(cmdWrapper.get('.rename-row button').attributes('disabled')).toBeDefined()
    expect(cmdWrapper.text()).toContain('An entry with this name already exists')
    await cmdWrapper.get('.rename-row input').setValue('/audit')
    await cmdWrapper.get('.rename-row input').trigger('blur')
    expect(Object.keys((cmdDoc.command as JsonObject) ?? {})).toEqual(['audit', 'deploy'])

    provWrapper.unmount()
    cmdWrapper.unmount()
    mcpWrapper.unmount()
    wrapper.unmount()
  })

  it('resets provider extra options without losing managed keys and clears invalid variant drafts on profile switch', async () => {
    const { wrapper, selectConfigFile } = await mountConfigDialog(
      {
        provider: {
          openai: {
            options: {
              apiKey: '{env:OPENAI_API_KEY}',
              baseURL: 'https://api.openai.com/v1',
              headers: { 'X-Team': 'core' },
              timeout: 45000,
            },
            models: {
              'gpt-5': { variants: { high: { reasoningEffort: 'high' } } },
            },
          },
        },
      },
      'providers',
    )

    await wrapper.get('.card .expand-btn').trigger('click')
    await wrapper.get('.model-card .expand-btn').trigger('click')

    // Click Reset on Extra SDK options -> removes `timeout` while keeping `apiKey`, `baseURL`, and `headers`.
    const extraRow = wrapper
      .findAll('.form-row')
      .find((row) => row.text().includes('Extra SDK options (JSON object)'))!
    await extraRow.get('.text-button').trigger('click')
    expect(
      (wrapper.get('textarea[aria-label="Extra SDK options (JSON object)"]').element as HTMLTextAreaElement)
        .value,
    ).toBe('')
    expect((wrapper.get('input[aria-label="Base URL"]').element as HTMLInputElement).value).toBe(
      'https://api.openai.com/v1',
    )

    // Break variant JSON, then switch profile -> triggers dirty guard; confirming Discard clears variant error.
    const variantArea = wrapper.get('textarea[aria-label="Variant options (JSON object)"]')
    await variantArea.setValue('{ invalid variant')
    expect(wrapper.text()).toContain('Must be a valid JSON object')
    expect(
      wrapper
        .findAll('.footer-actions button')
        .find((btn) => btn.text() === 'Save')!
        .attributes('disabled'),
    ).toBeDefined()

    const profileSelect = wrapper.get('select.profile-select')
    ;(profileSelect.element as HTMLSelectElement).value = 'ref.json.1.bak'
    await profileSelect.trigger('change')
    expect(wrapper.text()).toContain('Unsaved changes')

    await wrapper
      .findAll('.prompt-bar button')
      .find((btn) => btn.text() === 'Discard')!
      .trigger('click')
    await flushPromises()
    expect(selectConfigFile).toHaveBeenCalledWith('ref.json.1.bak', '/work')
    expect(wrapper.text()).not.toContain('Must be a valid JSON object')
    wrapper.unmount()
  })
})

describe('Dead i18n keys removal and disabled PermissionRulesEditor inertness', () => {
  it('removes unused config.editor.{apply,expand,collapse} from both en and zh-CN', () => {
    const enEditor = LOCALES.en.config.editor as Record<string, unknown>
    const zhEditor = LOCALES['zh-CN'].config.editor as Record<string, unknown>
    for (const deadKey of ['apply', 'expand', 'collapse']) {
      expect(enEditor[deadKey]).toBeUndefined()
      expect(zhEditor[deadKey]).toBeUndefined()
    }
  })

  it('sets inert on disabled PermissionRulesEditor and blocks mode activation', async () => {
    const onUpdate = vi.fn()
    const wrapper = mount(PermissionRulesEditor, {
      props: {
        modelValue: 'allow',
        disabled: true,
        'onUpdate:modelValue': onUpdate,
      },
      global: i18nGlobal(),
    })

    const root = wrapper.get('.permission-editor')
    expect((root.element as HTMLElement).inert).toBe(true)

    // Attempt to click Default or Rules radio buttons while disabled.
    const radios = wrapper.findAll('[role="radio"]')
    await radios[0]!.trigger('click')
    await radios[2]!.trigger('click')
    expect(onUpdate).not.toHaveBeenCalled()
  })
})

describe('Agents Default variant dropdown from configured and server model variants', () => {
  const serverModels: ModelInfo[] = [
    {
      provider: 'anthropic',
      provider_name: 'Anthropic',
      id: 'claude-sonnet-4-5',
      name: 'Claude Sonnet 4.5',
      context: 200000,
      output: 8192,
      images: true,
      reasoning: true,
      variants: ['medium', 'high', 'max'],
    },
    {
      provider: 'openrouter',
      provider_name: 'OpenRouter',
      id: 'anthropic/claude-opus-4',
      name: 'Claude Opus 4',
      context: 200000,
      output: 8192,
      images: true,
      reasoning: true,
      variants: ['low', 'high'],
    },
  ]

  it('resolves variants from doc + serverModels, strips #variant, and splits on first slash', () => {
    const doc: JsonObject = {
      model: 'anthropic/claude-sonnet-4-5#high',
      provider: {
        anthropic: {
          models: {
            'claude-sonnet-4-5': { variants: { custom: {}, high: {} } },
          },
        },
      },
    }

    expect(configuredVariantOptions(doc, '', serverModels)).toEqual([
      { value: 'custom', label: 'custom' },
      { value: 'high', label: 'high' },
      { value: 'medium', label: 'medium' },
      { value: 'max', label: 'max' },
    ])
    expect(configuredVariantOptions(doc, 'openrouter/anthropic/claude-opus-4#low', serverModels)).toEqual([
      { value: 'low', label: 'low' },
      { value: 'high', label: 'high' },
    ])
    expect(configuredVariantOptions({}, 'invalid-model-ref', serverModels)).toEqual([])
  })

  it('renders SelectField + custom input + Reset for built-in and custom agents and syncs changes cleanly', async () => {
    let doc: JsonObject = {
      model: 'anthropic/claude-sonnet-4-5',
      provider: {
        openai: { models: { 'gpt-5': { variants: { minimal: {}, reasoning: {} } } } },
      },
      agent: {
        reviewer: { description: 'Reviewer', model: 'openai/gpt-5' },
      },
    }

    const wrapper = mount(AgentsSection, {
      props: {
        doc,
        serverModels,
        'onUpdate:doc': (next: JsonObject) => {
          doc = next
          void wrapper.setProps({ doc: next })
        },
      },
      global: i18nGlobal(),
    })

    const cards = wrapper.findAll('.card')
    const buildCard = cards[0]!
    const reviewerCard = cards[cards.length - 1]!
    await buildCard.get('.expand-btn').trigger('click')
    await reviewerCard.get('.expand-btn').trigger('click')

    // Built-in `build` inherits top-level `anthropic/claude-sonnet-4-5` -> variants from serverModels.
    const buildVariantSelect = buildCard.get('select[aria-label="Default variant"]')
    expect(buildVariantSelect.findAll('option').map((o) => o.attributes('value'))).toEqual([
      '',
      'medium',
      'high',
      'max',
    ])
    await buildVariantSelect.setValue('high')
    expect((doc.agent as JsonObject).build).toEqual({ variant: 'high' })
    expect((buildCard.get('input[aria-label="Default variant"]').element as HTMLInputElement).value).toBe(
      'high',
    )

    // Reset button clears variant override on built-in agent and prunes empty agent entry.
    const buildVariantRow = buildCard
      .findAll('.form-row')
      .find((row) => row.text().includes('Default variant'))!
    await buildVariantRow.get('.text-button').trigger('click')
    expect((doc.agent as JsonObject).build).toBeUndefined()
    expect(buildVariantRow.find('.text-button').exists()).toBe(false)

    // Custom `reviewer` uses its own model override `openai/gpt-5` -> variants from doc, plus custom entry & reset.
    const reviewerVariantSelect = reviewerCard.get('select[aria-label="Default variant"]')
    expect(reviewerVariantSelect.findAll('option').map((o) => o.attributes('value'))).toEqual([
      '',
      'minimal',
      'reasoning',
    ])
    await reviewerCard.get('input[aria-label="Default variant"]').setValue('ultra')
    expect(((doc.agent as JsonObject).reviewer as JsonObject).variant).toBe('ultra')
    const reviewerVariantRow = reviewerCard
      .findAll('.form-row')
      .find((row) => row.text().includes('Default variant'))!
    await reviewerVariantRow.get('.text-button').trigger('click')
    expect(((doc.agent as JsonObject).reviewer as JsonObject).variant).toBeUndefined()
  })
})
