import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { registerBuiltinCommands } from '../../commands/builtin.ts'
import { IS_MAC } from '../../commands/platform.ts'
import DialogHost from '../../components/DialogHost.vue'
import { createAppI18n } from '../../i18n/index.ts'
import type { ConfigFilesInfo } from '../../proto/generated'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import { useShortcuts } from '../useShortcuts.ts'

function makeConfigInfo(): ConfigFilesInfo {
  const raw = '{\n  "$name": "Work"\n}\n'
  return {
    config_dir: '/home/user/.config/ref',
    files: [
      {
        file: 'ref.json',
        active: true,
        exists: true,
        label: 'Work',
        summary: '',
        modified_at: 1000,
        size: raw.length,
      },
    ],
    selected_file: 'ref.json',
    raw_text: raw,
    parsed: { $name: 'Work' },
    env_status: {},
    has_jsonc_override: false,
    env_config_override: null,
    project_overrides: [],
    warnings: [],
  }
}

function dispatchPaletteShortcut(): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key: 'p',
    code: 'KeyP',
    ctrlKey: !IS_MAC,
    metaKey: IS_MAC,
    bubbles: true,
    cancelable: true,
  })
  window.dispatchEvent(event)
  return event
}

describe('useShortcuts and dialogs closeGuard', () => {
  beforeEach(() => {
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
  })

  afterEach(() => {
    document.body.innerHTML = ''
    vi.unstubAllGlobals()
  })

  async function mountWithShortcuts() {
    const pinia = createPinia()
    setActivePinia(pinia)
    registerBuiltinCommands()

    const connection = useConnectionStore()
    const dialogs = useDialogsStore()
    const workspace = useWorkspaceStore()
    workspace.cwd = '/work'
    vi.spyOn(connection, 'getConfigFiles').mockResolvedValue(makeConfigInfo())

    const Root = defineComponent({
      setup() {
        useShortcuts()
        return () => h(DialogHost)
      },
    })

    const wrapper = mount(Root, {
      attachTo: document.body,
      global: { plugins: [pinia, createAppI18n(['en'])], stubs: { transition: false } },
    })
    await flushPromises()
    return { wrapper, dialogs }
  }

  it('toggles the command palette open and closed with Ctrl+P / Cmd+P when no other dialog is open', async () => {
    const { wrapper, dialogs } = await mountWithShortcuts()
    expect(dialogs.current).toBeUndefined()

    dispatchPaletteShortcut()
    await flushPromises()
    expect(dialogs.current).toEqual({ kind: 'palette' })

    dispatchPaletteShortcut()
    await flushPromises()
    expect(dialogs.current).toBeUndefined()
    wrapper.unmount()
  })

  it('does not let Ctrl+P / Cmd+P or programmatic dialogs.open bypass ConfigDialog closeGuard when dirty', async () => {
    const { wrapper, dialogs } = await mountWithShortcuts()
    dialogs.open({ kind: 'config' })
    await flushPromises()

    const nameInput = wrapper.get('input[placeholder="e.g. Work, Local"]')
    await nameInput.setValue('Unsaved Work')

    // Global Ctrl+P / Cmd+P shortcut must not replace an open ConfigDialog.
    dispatchPaletteShortcut()
    await flushPromises()
    expect(dialogs.current).toEqual({ kind: 'config' })
    expect((wrapper.get('input[placeholder="e.g. Work, Local"]').element as HTMLInputElement).value).toBe(
      'Unsaved Work',
    )

    // Programmatic dialogs.open() must also honor closeGuard and trigger the unsaved prompt.
    dialogs.open({ kind: 'palette' })
    await flushPromises()
    expect(dialogs.current).toEqual({ kind: 'config' })
    expect(wrapper.text()).toContain('Unsaved changes')
    wrapper.unmount()
  })
})
