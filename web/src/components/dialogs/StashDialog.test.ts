import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../../i18n/index.ts'
import { info } from '../../model/testMessages.ts'
import type { StashGroup } from '../../proto/generated'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import { ROOT_PALETTE } from '../../theme/agentColor.ts'
import StashDialog from './StashDialog.vue'

function group(id: string, overrides: Partial<StashGroup> = {}): StashGroup {
  return {
    id,
    name: `Group ${id}`,
    locked: false,
    created_at: 1,
    updated_at: 1,
    members: [],
    ...overrides,
  }
}

function mountStashDialog() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const workspace = useWorkspaceStore()
  const wrapper = mount(StashDialog, {
    attachTo: document.body,
    global: { plugins: [pinia, createAppI18n(['en'])] },
  })
  return { wrapper, workspace }
}

describe('StashDialog', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
  })

  it('shows running sessions with creator tint and no running badge, and group headers without active count', async () => {
    const { wrapper, workspace } = mountStashDialog()
    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        info({ id: 's1', name: 'lead', parent: null }),
        info({ id: 's2', name: 'worker', parent: 's1' }),
      ],
    })
    workspace.onFrame({
      kind: 'groups_changed',
      groups: [
        group('g1', {
          name: 'Work',
          members: [
            { session_id: 's1', position: 0, info: info({ id: 's1', name: 'lead', live: true }) },
            {
              session_id: 's3',
              position: 1,
              info: info({ id: 's3', name: 'archived', parent: 's1', live: false }),
            },
          ],
        }),
      ],
    })
    await nextTick()

    const runningCards = wrapper.findAll('.running-card')
    expect(runningCards).toHaveLength(2)
    expect(runningCards[0]!.text()).not.toMatch(/running/i)
    expect(runningCards[1]!.find('.tint-bar').attributes('style')).toBeDefined()

    const header = wrapper.get('.group-header')
    expect(header.text()).toBe('WorkRestore all')
    expect(wrapper.findAll('.member-row')[0]!.classes()).toContain('live')
    expect(wrapper.findAll('.member-row')[1]!.classes()).not.toContain('live')
    wrapper.unmount()
  })

  it('supports lock toggle, inline rename, restore all, restore member, delete, and drag-and-drop operations', async () => {
    const { wrapper, workspace } = mountStashDialog()
    const setLockSpy = vi.spyOn(workspace, 'setGroupLocked').mockResolvedValue(undefined)
    const renameSpy = vi.spyOn(workspace, 'renameGroup').mockResolvedValue(undefined)
    const restoreGroupSpy = vi.spyOn(workspace, 'restoreGroup').mockResolvedValue(['s3'])
    const restoreMemberSpy = vi.spyOn(workspace, 'restoreGroupMember').mockResolvedValue(true)
    const deleteSpy = vi.spyOn(workspace, 'deleteGroup').mockResolvedValue(undefined)
    const addSpy = vi.spyOn(workspace, 'addToGroup').mockResolvedValue(undefined)
    const createSpy = vi.spyOn(workspace, 'createGroup').mockResolvedValue(group('g_new'))
    const moveSpy = vi.spyOn(workspace, 'moveGroupMember').mockResolvedValue(undefined)
    const removeSpy = vi.spyOn(workspace, 'removeFromGroup').mockResolvedValue(undefined)
    const mergeSpy = vi.spyOn(workspace, 'mergeGroups').mockResolvedValue(undefined)

    workspace.onFrame({
      kind: 'sessions',
      sessions: [info({ id: 's_live', name: 'active' })],
    })
    workspace.onFrame({
      kind: 'groups_changed',
      groups: [
        group('g1', {
          name: 'Alpha',
          members: [{ session_id: 's3', position: 0, info: info({ id: 's3', name: 'm1', live: false }) }],
        }),
        group('g2', {
          name: 'Beta',
          members: [{ session_id: 's4', position: 0, info: info({ id: 's4', name: 'm2', live: false }) }],
        }),
      ],
    })
    await nextTick()

    // Lock toggle, rename, restore, delete.
    await wrapper.get('[aria-label="Lock group"]').trigger('click')
    expect(setLockSpy).toHaveBeenCalledWith('g1', true)

    await wrapper.get('.group-title').trigger('click')
    await nextTick()
    const input = wrapper.get('.group-title-input')
    await input.setValue('Renamed Alpha')
    await input.trigger('keydown', { key: 'Enter' })
    expect(renameSpy).toHaveBeenCalledWith('g1', 'Renamed Alpha')

    await wrapper.get('.group-actions .dialog-button').trigger('click')
    expect(restoreGroupSpy).toHaveBeenCalledWith('g1')

    await wrapper.get('[aria-label="Restore to background"]').trigger('click')
    expect(restoreMemberSpy).toHaveBeenCalledWith('g1', 's3')

    await wrapper.get('[aria-label="Delete group"]').trigger('click')
    expect(deleteSpy).toHaveBeenCalledWith('g1')

    // Drag running -> group g1.
    await wrapper.get('.running-card').trigger('dragstart')
    await wrapper.findAll('.group-card')[0]!.trigger('drop')
    expect(addSpy).toHaveBeenCalledWith('g1', 's_live', { stashLive: true })

    // Drag running -> + New group.
    await wrapper.get('.running-card').trigger('dragstart')
    await wrapper.get('.new-group-zone').trigger('drop')
    expect(createSpy).toHaveBeenCalledWith(['s_live'], { stashLive: true })

    // Drag member g1/s3 -> g2.
    await wrapper.findAll('.member-row')[0]!.trigger('dragstart')
    await wrapper.findAll('.group-card')[1]!.trigger('drop')
    expect(moveSpy).toHaveBeenCalledWith('g2', 's3', 1, { fromGroupId: 'g1', stashLive: false })

    // Drag member g1/s3 -> running column to restore.
    restoreMemberSpy.mockClear()
    removeSpy.mockClear()
    await wrapper.findAll('.member-row')[0]!.trigger('dragstart')
    await wrapper.get('.running-column').trigger('dragover')
    expect(wrapper.get('.running-column').classes()).toContain('drop-restore')
    await wrapper.get('.running-column').trigger('drop')
    expect(restoreMemberSpy).toHaveBeenCalledWith('g1', 's3')
    expect(removeSpy).not.toHaveBeenCalled()

    // Drag member g1/s3 -> groups column empty space does not remove.
    await wrapper.findAll('.member-row')[0]!.trigger('dragstart')
    await wrapper.get('.groups-column').trigger('dragover')
    expect(wrapper.get('.stash-modal').classes()).not.toContain('drop-remove')
    await wrapper.get('.groups-column').trigger('drop')
    expect(removeSpy).not.toHaveBeenCalled()

    // Drag member g1/s3 -> outside columns to remove.
    await wrapper.findAll('.member-row')[0]!.trigger('dragstart')
    await wrapper.get('.columns').trigger('dragover')
    expect(wrapper.get('.stash-modal').classes()).toContain('drop-remove')
    await wrapper.get('.columns').trigger('drop')
    expect(removeSpy).toHaveBeenCalledWith('g1', 's3')

    // Drag member g1/s3 -> window/scrim outside dialog also removes.
    removeSpy.mockClear()
    await wrapper.findAll('.member-row')[0]!.trigger('dragstart')
    window.dispatchEvent(new Event('dragover', { bubbles: true, cancelable: true }))
    await nextTick()
    expect(wrapper.get('.stash-modal').classes()).toContain('drop-remove')
    window.dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }))
    expect(removeSpy).toHaveBeenCalledWith('g1', 's3')

    // Drag group header g1 -> group g2 to merge.
    await wrapper.findAll('.group-header')[0]!.trigger('dragstart')
    await wrapper.findAll('.group-card')[1]!.trigger('drop')
    expect(mergeSpy).toHaveBeenCalledWith('g1', 'g2')

    wrapper.unmount()
  })

  it('handles same-group upward and downward member reordering without off-by-one, and supports move buttons', async () => {
    const { wrapper, workspace } = mountStashDialog()
    const moveSpy = vi.spyOn(workspace, 'moveGroupMember').mockResolvedValue(undefined)
    workspace.onFrame({
      kind: 'groups_changed',
      groups: [
        group('g1', {
          name: 'Work',
          members: [
            { session_id: 's1', position: 0, info: info({ id: 's1', name: 'first', live: false }) },
            { session_id: 's2', position: 1, info: info({ id: 's2', name: 'second', live: false }) },
            { session_id: 's3', position: 2, info: info({ id: 's3', name: 'third', live: false }) },
          ],
        }),
      ],
    })
    await nextTick()

    const rows = wrapper.findAll('.member-row')
    // Downward drag: s1 (index 0) dropped before s3 (index 2) -> target position 1.
    await rows[0]!.trigger('dragstart')
    await rows[2]!.trigger('drop')
    expect(moveSpy).toHaveBeenLastCalledWith('g1', 's1', 1, { fromGroupId: 'g1', stashLive: false })

    // Upward drag: s3 (index 2) dropped before s1 (index 0) -> target position 0.
    await rows[2]!.trigger('dragstart')
    await rows[0]!.trigger('drop')
    expect(moveSpy).toHaveBeenLastCalledWith('g1', 's3', 0, { fromGroupId: 'g1', stashLive: false })

    // Keyboard-accessible move up/down buttons.
    await rows[0]!.get('[aria-label="Move down"]').trigger('click')
    expect(moveSpy).toHaveBeenLastCalledWith('g1', 's1', 1, { fromGroupId: 'g1', stashLive: false })
    await rows[2]!.get('[aria-label="Move up"]').trigger('click')
    expect(moveSpy).toHaveBeenLastCalledWith('g1', 's3', 1, { fromGroupId: 'g1', stashLive: false })

    // Renaming disables group header dragging.
    await wrapper.get('.group-title').trigger('click')
    await nextTick()
    expect(wrapper.get('.group-header').attributes('draggable')).toBe('false')
    wrapper.unmount()
  })

  it('assigns conflict-free 7-palette tints across colliding live sessions and stashed members', async () => {
    const { wrapper, workspace } = mountStashDialog()
    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        info({ id: 'ses_0', name: 'root-a', parent: null }),
        info({ id: 'ses_0_c', name: 'child-a', parent: 'ses_0' }),
        info({ id: 'ses_7', name: 'root-b', parent: null }),
      ],
    })
    workspace.onFrame({
      kind: 'groups_changed',
      groups: [
        group('g1', {
          name: 'Work',
          members: [
            {
              session_id: 'ses_E',
              position: 0,
              info: info({ id: 'ses_E', name: 'root-c', parent: null, live: false }),
            },
            {
              session_id: 'ses_7_m',
              position: 1,
              info: info({ id: 'ses_7_m', name: 'child-b', parent: 'ses_7', live: false }),
            },
          ],
        }),
      ],
    })
    await nextTick()

    const runningCards = wrapper.findAll('.running-card')
    const memberRows = wrapper.findAll('.member-row')
    const tint0 = runningCards[0]!.get('.tint-bar').attributes('style')
    const tint0Child = runningCards[1]!.get('.tint-bar').attributes('style')
    const tint7 = runningCards[2]!.get('.tint-bar').attributes('style')
    const tintE = memberRows[0]!.get('.tint-bar').attributes('style')
    const tint7Member = memberRows[1]!.get('.tint-bar').attributes('style')

    expect(tint0).toBeDefined()
    expect(tint0Child).toBe(tint0)
    expect(tint7Member).toBe(tint7)
    expect(new Set([tint0, tint7, tintE]).size).toBe(3)
    for (const style of [tint0, tint7, tintE]) {
      expect(ROOT_PALETTE.some((color) => style?.includes(color))).toBe(true)
    }
    wrapper.unmount()
  })

  it('reorders running sessions via drag-and-drop and restores stashed members at exact drop slot with pin clamping', async () => {
    const { wrapper, workspace } = mountStashDialog()
    const moveRunningSpy = vi.spyOn(workspace, 'moveRunningSession')
    const restoreMemberSpy = vi.spyOn(workspace, 'restoreGroupMember').mockResolvedValue(true)

    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        info({ id: 's_pin', name: 'pinned', pinned_at: 100, created_at: 1 }),
        info({ id: 's1', name: 'first', created_at: 2 }),
        info({ id: 's2', name: 'second', created_at: 3 }),
      ],
    })
    workspace.onFrame({
      kind: 'groups_changed',
      groups: [
        group('g1', {
          name: 'Work',
          members: [
            {
              session_id: 'm_unpinned',
              position: 0,
              info: info({ id: 'm_unpinned', name: 'archived-u', live: false }),
            },
            {
              session_id: 'm_pinned',
              position: 1,
              info: info({ id: 'm_pinned', name: 'archived-p', live: false, pinned_at: 200 }),
            },
          ],
        }),
      ],
    })
    await nextTick()

    const mockRunningCardRects = () => {
      const cards = wrapper.findAll('.running-card')
      cards.forEach((card, idx) => {
        const top = 100 + idx * 46
        vi.spyOn(card.element, 'getBoundingClientRect').mockReturnValue({
          x: 20,
          y: top,
          left: 20,
          right: 260,
          top,
          bottom: top + 40,
          width: 240,
          height: 40,
          toJSON: () => ({}),
        } as DOMRect)
      })
      return cards
    }

    let cards = mockRunningCardRects()
    const runningCol = wrapper.get('.running-column')

    // No-op drag on s1 bottom half (insertIndex=2 -> toIndex=1 === fromIndex) shows no indicator.
    await cards[1]!.trigger('dragstart')
    await runningCol.trigger('dragover', { clientX: 50, clientY: 178 })
    expect(runningCol.classes()).not.toContain('drop-restore')
    expect(cards[1]!.classes()).not.toContain('drop-before')
    expect(cards[2]!.classes()).not.toContain('drop-before')
    await runningCol.trigger('drop', { clientX: 50, clientY: 178 })
    expect(moveRunningSpy).not.toHaveBeenCalled()

    // Downward drag: s1 (index 1) onto bottom half of s2 (index 2) -> target index 2.
    await cards[1]!.trigger('dragstart')
    await runningCol.trigger('dragover', { clientX: 50, clientY: 224 })
    expect(cards[2]!.classes()).toContain('drop-after')
    await runningCol.trigger('drop', { clientX: 50, clientY: 224 })
    expect(moveRunningSpy).toHaveBeenCalledWith('s1', 2)
    await nextTick()
    expect(workspace.live.map((s) => s.id)).toEqual(['s_pin', 's2', 's1'])

    // Upward drag: s1 (now index 2) onto top half of s_pin (index 0) clamps to pinnedCount=1.
    cards = mockRunningCardRects()
    await cards[2]!.trigger('dragstart')
    await runningCol.trigger('dragover', { clientX: 50, clientY: 108 })
    expect(cards[1]!.classes()).toContain('drop-before')
    await runningCol.trigger('drop', { clientX: 50, clientY: 108 })
    expect(moveRunningSpy).toHaveBeenLastCalledWith('s1', 1)
    await nextTick()
    expect(workspace.live.map((s) => s.id)).toEqual(['s_pin', 's1', 's2'])

    // Drag unpinned member onto top half of s2 (index 2) -> targetIndex: 2, pinned: false.
    cards = mockRunningCardRects()
    const memberRows = wrapper.findAll('.member-row')
    await memberRows[0]!.trigger('dragstart')
    await runningCol.trigger('dragover', { clientX: 50, clientY: 200 })
    expect(runningCol.classes()).toContain('drop-restore')
    expect(cards[2]!.classes()).toContain('drop-before')
    await runningCol.trigger('drop', { clientX: 50, clientY: 200 })
    expect(restoreMemberSpy).toHaveBeenLastCalledWith('g1', 'm_unpinned', {
      targetIndex: 2,
      pinned: false,
    })

    // Drag pinned member below list -> clamped to pinnedCount (1), pinned: true.
    await memberRows[1]!.trigger('dragstart')
    await runningCol.trigger('dragover', { clientX: 50, clientY: 260 })
    expect(cards[1]!.classes()).toContain('drop-before')
    await runningCol.trigger('drop', { clientX: 50, clientY: 260 })
    expect(restoreMemberSpy).toHaveBeenLastCalledWith('g1', 'm_pinned', {
      targetIndex: 1,
      pinned: true,
    })

    wrapper.unmount()
  })
})
