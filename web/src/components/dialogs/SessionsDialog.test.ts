import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../../i18n/index.ts'
import { info } from '../../model/testMessages.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import SessionsDialog from './SessionsDialog.vue'

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
})
