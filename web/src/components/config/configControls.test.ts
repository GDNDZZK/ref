import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../../i18n/index.ts'
import {
  ColorSwatchPicker,
  FormRow,
  KeyValueEditor,
  ModelField,
  NumberField,
  PermissionRulesEditor,
  SecretField,
  SelectField,
  StringListEditor,
  TextAreaField,
} from './index.ts'

const mounted: VueWrapper[] = []

afterEach(() => {
  mounted.splice(0).forEach((wrapper) => wrapper.unmount())
  document.body.innerHTML = ''
})
function i18nGlobal(locale = 'en') {
  return { plugins: [createAppI18n([locale])] }
}

describe('FormRow', () => {
  it('renders label, description, vertical layout, error alert, and reset button', async () => {
    const onReset = vi.fn()
    const wrapper = mount(FormRow, {
      props: {
        label: 'System prompt',
        description: 'Prepended to the agent instructions.',
        vertical: true,
        canReset: true,
        error: 'Prompt cannot be empty',
        onReset,
      },
      slots: { default: '<input class="slot-control" />' },
      global: i18nGlobal(),
    })

    expect(wrapper.classes()).toContain('vertical')
    expect(wrapper.text()).toContain('System prompt')
    expect(wrapper.text()).toContain('Prepended to the agent instructions.')
    expect(wrapper.find('[role="alert"]').text()).toBe('Prompt cannot be empty')
    expect(wrapper.find('.slot-control').exists()).toBe(true)

    await wrapper.get('button').trigger('click')
    expect(onReset).toHaveBeenCalledOnce()
  })
})

describe('SelectField', () => {
  it('binds v-model and preserves custom ref.json values not present in options', async () => {
    let model = 'custom-provider/my-model'
    const wrapper = mount(SelectField, {
      props: {
        modelValue: model,
        options: [
          { value: 'anthropic/claude-sonnet-4-5', label: 'Claude Sonnet 4.5' },
          { value: 'openai/gpt-5', label: 'GPT-5' },
        ],
        placeholder: 'Default model',
        label: 'Model',
        'onUpdate:modelValue': (next: string) => {
          model = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      global: i18nGlobal(),
    })

    const select = wrapper.get('select')
    expect((select.element as HTMLSelectElement).value).toBe('custom-provider/my-model')
    expect(wrapper.findAll('option').map((opt) => opt.attributes('value'))).toEqual([
      '',
      'anthropic/claude-sonnet-4-5',
      'openai/gpt-5',
      'custom-provider/my-model',
    ])

    await select.setValue('openai/gpt-5')
    expect(model).toBe('openai/gpt-5')
  })

  it('supports editable combobox filtering, keyboard navigation, Escape propagation stop, and custom input', async () => {
    let model = 'anthropic/claude-sonnet-4-5'
    const wrapper = mount(SelectField, {
      props: {
        modelValue: model,
        editable: true,
        monospace: true,
        options: [
          { value: 'anthropic/claude-sonnet-4-5', label: 'Claude Sonnet 4.5 (anthropic/claude-sonnet-4-5)' },
          { value: 'openai/gpt-5', label: 'GPT-5 (openai/gpt-5)' },
          { value: 'google/gemini-2.5', label: 'Gemini 2.5 (google/gemini-2.5)', disabled: true },
          { value: 'xai/grok-4', label: 'Grok 4 (xai/grok-4)' },
        ],
        placeholder: 'provider/model',
        label: 'Default model',
        'onUpdate:modelValue': (next: string) => {
          model = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      attachTo: document.body,
      global: i18nGlobal(),
    })
    mounted.push(wrapper)

    expect(wrapper.find('select').exists()).toBe(false)
    const input = wrapper.get('input[role="combobox"]')
    const toggle = wrapper.get('button.toggle')
    expect(input.classes()).toContain('mono')
    expect(input.attributes('aria-expanded')).toBe('false')
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()

    // Clicking chevron toggle shows ALL options even when input holds a committed value.
    await toggle.trigger('click')
    expect(input.attributes('aria-expanded')).toBe('true')
    const listbox = document.body.querySelector('[role="listbox"]')
    expect(listbox).not.toBeNull()
    expect(listbox!.querySelectorAll('[role="option"]')).toHaveLength(4)

    // Escape closes dropdown and stops propagation so parent dialog listeners do not fire.
    const parentEsc = vi.fn()
    document.body.addEventListener('keydown', parentEsc)
    await input.trigger('keydown', { key: 'Escape' })
    document.body.removeEventListener('keydown', parentEsc)
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()
    expect(input.attributes('aria-expanded')).toBe('false')
    expect(parentEsc).not.toHaveBeenCalled()

    // Typing filters options by label + value and updates modelValue immediately.
    await input.setValue('gpt')
    expect(model).toBe('gpt')
    const filteredOptions = document.body.querySelectorAll('[role="option"]')
    expect(filteredOptions).toHaveLength(1)
    expect(filteredOptions[0]!.textContent?.trim()).toBe('GPT-5 (openai/gpt-5)')

    // Clearing input shows all options again.
    await input.setValue('')
    expect(model).toBe('')
    expect(document.body.querySelectorAll('[role="option"]')).toHaveLength(4)

    // ArrowDown / ArrowUp cycle highlight skipping disabled items, and Enter picks the highlighted option.
    await input.trigger('keydown', { key: 'Escape' })
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()

    await input.trigger('keydown', { key: 'ArrowDown' }) // opens & highlights index 0 (Claude)
    expect(document.body.querySelector('[role="listbox"]')).not.toBeNull()
    await input.trigger('keydown', { key: 'ArrowDown' }) // index 1 (GPT-5)
    await input.trigger('keydown', { key: 'ArrowDown' }) // skips disabled index 2 -> index 3 (Grok 4)
    await input.trigger('keydown', { key: 'ArrowUp' }) // skips disabled index 2 -> index 1 (GPT-5)
    await input.trigger('keydown', { key: 'Enter' })
    expect(model).toBe('openai/gpt-5')
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()
    expect((input.element as HTMLInputElement).value).toBe('openai/gpt-5')

    // Typing a custom non-matching value updates modelValue and leaves listbox closed.
    await input.setValue('custom/local-model')
    expect(model).toBe('custom/local-model')
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()

    // Clicking an option selects it and closes the dropdown; clicking outside also closes.
    await toggle.trigger('click')
    expect(document.body.querySelector('[role="listbox"]')).not.toBeNull()
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    await wrapper.vm.$nextTick()
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()

    await toggle.trigger('click')
    const firstOption = document.body.querySelectorAll('[role="option"]')[0] as HTMLElement
    firstOption.click()
    await wrapper.vm.$nextTick()
    expect(model).toBe('anthropic/claude-sonnet-4-5')
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()
  })
})

describe('ModelField', () => {
  it('renders a single editable monospace combobox and omits toggle when options are empty', async () => {
    let model = ''
    const wrapper = mount(ModelField, {
      props: {
        modelValue: model,
        options: [
          { value: 'anthropic/claude-sonnet-4-5', label: 'Claude Sonnet 4.5 (anthropic/claude-sonnet-4-5)' },
          { value: 'openai/gpt-5', label: 'GPT-5 (openai/gpt-5)' },
        ],
        label: 'Default model',
        'onUpdate:modelValue': (next: string) => {
          model = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      attachTo: document.body,
      global: i18nGlobal(),
    })
    mounted.push(wrapper)

    expect(wrapper.find('select').exists()).toBe(false)
    expect(wrapper.findAll('input')).toHaveLength(1)
    const input = wrapper.get('input[role="combobox"]')
    expect(input.classes()).toContain('mono')
    expect(wrapper.find('button.toggle').exists()).toBe(true)

    await wrapper.get('button.toggle').trigger('click')
    const options = document.body.querySelectorAll('[role="option"]')
    expect(options).toHaveLength(2)
    ;(options[1] as HTMLElement).click()
    await wrapper.vm.$nextTick()
    expect(model).toBe('openai/gpt-5')

    await input.setValue('ollama/qwen3')
    expect(model).toBe('ollama/qwen3')

    await wrapper.setProps({ options: [] })
    expect(wrapper.find('button.toggle').exists()).toBe(false)
  })
})

describe('SecretField', () => {
  it('renders {env:VAR} and {file:...} in plain text with live envStatus badges', async () => {
    const wrapper = mount(SecretField, {
      props: {
        modelValue: '{env:ANTHROPIC_API_KEY} {env:MISSING_KEY}',
        envStatus: { ANTHROPIC_API_KEY: true, MISSING_KEY: false },
        label: 'API key',
      },
      global: i18nGlobal(),
    })

    expect(wrapper.get('input').attributes('type')).toBe('text')
    expect(wrapper.find('button.toggle').exists()).toBe(false)

    const badges = wrapper.findAll('.badge')
    expect(badges).toHaveLength(2)
    expect(badges[0]!.classes()).toContain('set')
    expect(badges[0]!.text()).toBe('✓ ANTHROPIC_API_KEY set')
    expect(badges[1]!.classes()).toContain('unset')
    expect(badges[1]!.text()).toBe('⚠ MISSING_KEY unset')

    await wrapper.setProps({ modelValue: '{file:~/.config/ref/key}' })
    expect(wrapper.get('input').attributes('type')).toBe('text')
    expect(wrapper.find('button.toggle').exists()).toBe(false)
  })

  it('masks literal API keys by default and toggles Show / Hide', async () => {
    const wrapper = mount(SecretField, {
      props: {
        modelValue: 'sk-ant-literal-secret',
        label: 'API key',
      },
      global: i18nGlobal(),
    })

    const input = wrapper.get('input')
    const toggle = wrapper.get('button.toggle')

    expect(input.attributes('type')).toBe('password')
    expect(toggle.text()).toBe('Show')

    await toggle.trigger('click')
    expect(input.attributes('type')).toBe('text')
    expect(toggle.text()).toBe('Hide')

    await toggle.trigger('click')
    expect(input.attributes('type')).toBe('password')
  })

  it('renders Chinese badge and toggle labels under zh-CN', () => {
    const wrapper = mount(SecretField, {
      props: {
        modelValue: '{env:OPENAI_API_KEY}',
        envStatus: { OPENAI_API_KEY: true },
      },
      global: i18nGlobal('zh-CN'),
    })
    expect(wrapper.get('.badge').text()).toBe('✓ OPENAI_API_KEY 已设置')
  })
})

describe('NumberField', () => {
  it('emits numbers for valid numeric input including 0 and undefined when cleared', async () => {
    let count: number | undefined = 200000
    const wrapper = mount(NumberField, {
      props: {
        modelValue: count,
        min: 0,
        label: 'Context tokens',
        'onUpdate:modelValue': (next: number | undefined) => {
          count = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      global: i18nGlobal(),
    })

    const input = wrapper.get('input')
    expect((input.element as HTMLInputElement).value).toBe('200000')

    await input.setValue('0')
    expect(count).toBe(0)

    await input.setValue('0.85')
    expect(count).toBe(0.85)

    await input.setValue('')
    expect(count).toBeUndefined()
  })
})

describe('TextAreaField', () => {
  it('supports v-model, monospace styling, and validation errors', async () => {
    let text = '{\n  "model": "openai/gpt-5"\n}'
    const wrapper = mount(TextAreaField, {
      props: {
        modelValue: text,
        monospace: true,
        error: 'Unexpected token',
        label: 'JSON',
        'onUpdate:modelValue': (next: string) => {
          text = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      global: i18nGlobal(),
    })

    const area = wrapper.get('textarea')
    expect(area.classes()).toContain('mono')
    expect(wrapper.get('[role="alert"]').text()).toBe('Unexpected token')

    await area.setValue('{}')
    expect(text).toBe('{}')
  })
})

describe('StringListEditor', () => {
  it('adds items via Enter or button, edits inline, reorders, and removes items', async () => {
    let items = ['npx', '-y']
    const wrapper = mount(StringListEditor, {
      props: {
        modelValue: items,
        monospace: true,
        placeholder: 'Argument',
        'onUpdate:modelValue': (next: string[]) => {
          items = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      global: i18nGlobal(),
    })

    const addInput = wrapper.get('.add-row input')
    await addInput.setValue('@modelcontextprotocol/server-github')
    await addInput.trigger('keydown', { key: 'Enter' })
    await wrapper.vm.$nextTick()
    expect(items).toEqual(['npx', '-y', '@modelcontextprotocol/server-github'])

    // Move "-y" up to index 0.
    const secondRowButtons = wrapper.findAll('.item-row')[1]!.findAll('button')
    await secondRowButtons[0]!.trigger('click')
    await wrapper.vm.$nextTick()
    expect(items).toEqual(['-y', 'npx', '@modelcontextprotocol/server-github'])

    // Inline-edit index 0.
    await wrapper.findAll('.item-row input')[0]!.setValue('--yes')
    await wrapper.vm.$nextTick()
    expect(items[0]).toBe('--yes')

    // Remove index 1 ("npx").
    await wrapper.findAll('.item-row')[1]!.findAll('button')[2]!.trigger('click')
    expect(items).toEqual(['--yes', '@modelcontextprotocol/server-github'])
  })
})

describe('KeyValueEditor', () => {
  it('preserves exact DOM input identity while typing keys and supports SecretField values', async () => {
    let headers: Record<string, string> = {
      'X-Org': 'acme',
      Authorization: 'Bearer literal-token',
    }
    const wrapper = mount(KeyValueEditor, {
      props: {
        modelValue: headers,
        envStatus: { DOCS_TOKEN: true },
        'onUpdate:modelValue': (next: Record<string, string>) => {
          headers = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      global: i18nGlobal(),
    })

    const rows = wrapper.findAll('.kv-row')
    expect(rows).toHaveLength(2)

    // Plain header ("X-Org") stays unmasked text; "Authorization" literal value is masked!
    expect(rows[0]!.get('.secret-field input').attributes('type')).toBe('text')
    expect(rows[1]!.get('.secret-field input').attributes('type')).toBe('password')

    // Add a new entry and type its key across multiple keystrokes without remounting the input!
    await wrapper.get('.footer button').trigger('click')
    const addedKeyInput = wrapper.findAll('.kv-row')[2]!.get('.key-input')
    const originalElement = addedKeyInput.element

    await addedKeyInput.setValue('X')
    await addedKeyInput.setValue('X-Team')
    expect(wrapper.findAll('.kv-row')[2]!.get('.key-input').element).toBe(originalElement)

    await wrapper.findAll('.kv-row')[2]!.get('.secret-field input').setValue('Bearer {env:DOCS_TOKEN}')
    expect(headers).toEqual({
      'X-Org': 'acme',
      Authorization: 'Bearer literal-token',
      'X-Team': 'Bearer {env:DOCS_TOKEN}',
    })
    expect(wrapper.findAll('.kv-row')[2]!.get('.badge').text()).toBe('✓ DOCS_TOKEN set')

    // Remove the first row ("X-Org").
    await wrapper.findAll('.kv-row')[0]!.get('.remove-btn').trigger('click')
    expect(headers).toEqual({
      Authorization: 'Bearer literal-token',
      'X-Team': 'Bearer {env:DOCS_TOKEN}',
    })
  })
})

describe('PermissionRulesEditor', () => {
  it('switches cleanly across Default, Global, and Rule list modes and serializes rules losslessly', async () => {
    let perm: unknown = {
      '*': 'allow',
      bash: { '*': 'ask', 'git status*': 'allow' },
    }
    const wrapper = mount(PermissionRulesEditor, {
      props: {
        modelValue: perm,
        'onUpdate:modelValue': (next: unknown) => {
          perm = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      global: i18nGlobal(),
    })

    expect(wrapper.findAll('.rule-row')).toHaveLength(3)

    // Add a new rule and customize it.
    await wrapper.get('.rules-footer button').trigger('click')
    const ruleRows = wrapper.findAll('.rule-row')
    expect(ruleRows).toHaveLength(4)
    await ruleRows[3]!.get('.tool-input').setValue('mcp_github_*')
    expect(perm).toEqual({
      '*': 'allow',
      bash: { '*': 'ask', 'git status*': 'allow' },
      'mcp_github_*': 'ask',
    })

    // Reorder `git status*` above `bash -> *` and verify serialized pattern order updates!
    const thirdRowButtons = wrapper.findAll('.rule-row')[2]!.findAll('button.icon-btn')
    await thirdRowButtons[0]!.trigger('click')
    expect(Object.keys((perm as Record<string, Record<string, string>>).bash!)).toEqual(['git status*', '*'])

    // Switch to Global mode via Segmented radio buttons.
    const modeButtons = wrapper.findAll('[role="radio"]')
    await modeButtons[1]!.trigger('click')
    expect(perm).toBe('allow')

    await wrapper.get('.global-panel select').setValue('deny')
    expect(perm).toBe('deny')

    // Switch to Default (unset) mode -> emits undefined.
    await modeButtons[0]!.trigger('click')
    expect(perm).toBeUndefined()
  })
})

describe('ColorSwatchPicker', () => {
  it('picks named theme swatches, previews custom #rrggbb hex values, and clears to empty', async () => {
    let color = ''
    const wrapper = mount(ColorSwatchPicker, {
      props: {
        modelValue: color,
        label: 'Agent color',
        'onUpdate:modelValue': (next: string) => {
          color = next
          void wrapper.setProps({ modelValue: next })
        },
      },
      global: i18nGlobal(),
    })

    expect(wrapper.find('.auto-hint').text()).toBe('Auto')

    const swatches = wrapper.findAll('button.swatch')
    expect(swatches).toHaveLength(7)
    expect(swatches[1]!.attributes('style')).toContain('var(--agent-blue)')

    await swatches[1]!.trigger('click')
    expect(color).toBe('blue')
    expect(swatches[1]!.classes()).toContain('active')

    await wrapper.get('.hex-input').setValue('#e5c07b')
    expect(color).toBe('#e5c07b')
    expect(wrapper.find('.hex-preview').exists()).toBe(true)

    // Click Clear button -> resets to empty string ("Auto").
    await wrapper.get('.text-button').trigger('click')
    expect(color).toBe('')
    expect(wrapper.find('.auto-hint').exists()).toBe(true)
  })
})
