import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../../i18n/index.ts'
import { info } from '../../model/testMessages.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import SessionsDialog from './SessionsDialog.vue'
import AgentsDialog from './AgentsDialog.vue'
import ModelsDialog from './ModelsDialog.vue'

function mountSessionsDialog() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const workspace = useWorkspaceStore()
  workspace.onFrame({
    kind: 'sessions',
    sessions: [
      info({ id: 'ses_lead', name: 'lead', parent: null, created_at: 1 }),
      info({ id: 'ses_other', name: 'solo', parent: null, created_at: 2 }),
      info({ id: 'ses_w1', name: 'worker-1', parent: 'ses_lead', created_at: 3 }),
      info({ id: 'ses_w2', name: 'worker-2', parent: 'ses_lead', created_at: 4 }),
      info({ id: 'ses_sub', name: 'sub-task', parent: 'ses_other', created_at: 5 }),
    ],
  })
  const wrapper = mount(SessionsDialog, {
    global: { plugins: [pinia, createAppI18n(['en'])] },
  })
  return { wrapper, workspace }
}

function rowNames(wrapper: ReturnType<typeof mountSessionsDialog>['wrapper']): string[] {
  return wrapper.findAll('.row .name').map((node) => node.text())
}

describe('SessionsDialog', () => {
  beforeEach(() => {
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
  })

  it('cycles All, User-created, and By creator of selected row', async () => {
    const { wrapper } = mountSessionsDialog()
    expect(rowNames(wrapper)).toEqual(['lead', 'solo', 'worker-1', 'worker-2', 'sub-task'])

    const chips = wrapper.findAll('.filter-chip')
    expect(chips.map((c) => c.text())).toEqual(['All', 'User-created', 'By creator'])

    await chips[1]!.trigger('click')
    expect(rowNames(wrapper)).toEqual(['lead', 'solo'])

    await chips[0]!.trigger('click')
    expect(rowNames(wrapper)).toEqual(['lead', 'solo', 'worker-1', 'worker-2', 'sub-task'])

    // Move selection to worker-1 (created by ses_lead) and filter by its creator.
    await wrapper.findAll('[role="option"]')[2]!.trigger('mousemove')
    await nextTick()
    await chips[2]!.trigger('click')
    expect(rowNames(wrapper)).toEqual(['worker-1', 'worker-2'])
    expect(chips[2]!.text()).toBe('By lead')
  })

  it('matches creator name and creator id in search keywords', async () => {
    const { wrapper } = mountSessionsDialog()
    const input = wrapper.get('input.field')

    await input.setValue('solo')
    expect(rowNames(wrapper)).toEqual(['solo', 'sub-task'])

    await input.setValue('ses_lead')
    expect(rowNames(wrapper)).toEqual(['lead', 'worker-1', 'worker-2'])
  })

  it('opens closeSession confirmation for working sessions and closes idle ones directly', async () => {
    const { wrapper, workspace } = mountSessionsDialog()
    const dialogs = useDialogsStore()
    const closeSpy = vi.spyOn(workspace, 'closeSession').mockResolvedValue(undefined)
    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        info({ id: 'ses_busy', name: 'busy', status: { state: 'running' }, created_at: 1 }),
        info({ id: 'ses_idle', name: 'idle', status: { state: 'idle' }, created_at: 2 }),
      ],
    })
    await nextTick()

    const closeButtons = wrapper.findAll('[role="option"] button.close')
    await closeButtons[0]!.trigger('click')
    expect(dialogs.current).toEqual({ kind: 'closeSession', sessionId: 'ses_busy' })
    expect(closeSpy).not.toHaveBeenCalled()

    dialogs.close()
    await closeButtons[1]!.trigger('click')
    expect(dialogs.current).toBeUndefined()
    expect(closeSpy).toHaveBeenCalledWith('ses_idle')
  })

  it('shows only live sessions and closes selected row in close mode', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const workspace = useWorkspaceStore()
    const closeSpy = vi.spyOn(workspace, 'closeSession').mockResolvedValue(undefined)
    workspace.onFrame({
      kind: 'sessions',
      sessions: [info({ id: 'ses_live', name: 'live-one', status: { state: 'idle' } })],
    })
    workspace.history = [info({ id: 'ses_past', name: 'past-one', live: false })]
    const wrapper = mount(SessionsDialog, {
      props: { mode: 'close' },
      global: { plugins: [pinia, createAppI18n(['en'])] },
    })
    expect(rowNames(wrapper)).toEqual(['live-one'])
    await wrapper.get('[role="option"]').trigger('click')
    expect(closeSpy).toHaveBeenCalledWith('ses_live')
  })

  it('opens ConfigDialog at providers from ModelsDialog and agents from AgentsDialog', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const dialogs = useDialogsStore()
    const i18n = createAppI18n(['en'])

    const modelsWrapper = mount(ModelsDialog, {
      props: { sessionId: 's1' },
      global: { plugins: [pinia, i18n] },
    })
    await modelsWrapper.get('button.text-button').trigger('click')
    expect(dialogs.current).toEqual({ kind: 'config', section: 'providers' })

    const agentsWrapper = mount(AgentsDialog, {
      props: { sessionId: 's1' },
      global: { plugins: [pinia, i18n] },
    })
    await agentsWrapper.get('button.text-button').trigger('click')
    expect(dialogs.current).toEqual({ kind: 'config', section: 'agents' })
  })
})
