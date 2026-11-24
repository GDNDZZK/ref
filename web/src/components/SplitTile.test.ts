import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { describe, expect, it } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { info } from '../model/testMessages.ts'
import type { SessionInfo } from '../proto/generated'
import { useWorkspaceStore } from '../stores/workspace.ts'
import SessionDot from './SessionDot.vue'
import SplitTile from './SplitTile.vue'

const PaneStub = {
  props: ['composerShown'],
  methods: { focusComposer() {} },
  template: '<div class="pane"><button class="allow" /><div class="composer-area"><textarea /></div></div>',
}

function mountTile(options: {
  focused: boolean
  zoomed?: boolean
  sessionInfo?: SessionInfo
  live?: SessionInfo[]
}) {
  const pinia = createPinia()
  setActivePinia(pinia)
  if (options.live) {
    useWorkspaceStore().onFrame({ kind: 'sessions', sessions: options.live })
  }
  const sessionInfo = options.sessionInfo ?? info({ id: 's1' })
  const session = { id: sessionInfo.id, info: sessionInfo } as unknown as ChatSession
  return mount(SplitTile, {
    props: {
      session,
      focused: options.focused,
      zoomed: options.zoomed ?? false,
      hidden: false,
      clearance: { left: 0, right: 0 },
    },
    global: { plugins: [pinia, createAppI18n(['en'])], stubs: { SessionPane: PaneStub } },
  })
}

describe('SplitTile', () => {
  it('shows the composer only while the tile is focused or zoomed', () => {
    const pane = (wrapper: ReturnType<typeof mountTile>) => wrapper.getComponent(PaneStub)
    expect(pane(mountTile({ focused: false })).props('composerShown')).toBe(false)
    expect(pane(mountTile({ focused: true })).props('composerShown')).toBe(true)
    expect(pane(mountTile({ focused: false, zoomed: true })).props('composerShown')).toBe(true)
  })

  it('takes the focus on a press, but on a button only after its click', async () => {
    const wrapper = mountTile({ focused: false })
    await wrapper.get('.pane').trigger('pointerdown')
    expect(wrapper.emitted('focus')).toHaveLength(1)

    const button = wrapper.get('.allow')
    await button.trigger('pointerdown')
    expect(wrapper.emitted('focus')).toHaveLength(1)
    await button.trigger('click')
    expect(wrapper.emitted('focus')).toHaveLength(2)
  })

  it('emits unfocus when clicking an already-focused tile outside interactive controls', async () => {
    const wrapper = mountTile({ focused: true })
    await wrapper.get('.title').trigger('pointerdown')
    await wrapper.get('.title').trigger('click')
    expect(wrapper.emitted('unfocus')).toHaveLength(1)

    await wrapper.get('.allow').trigger('click')
    await wrapper.get('.composer-area textarea').trigger('click')
    expect(wrapper.emitted('unfocus')).toHaveLength(1)
  })

  it('leaves user-created tiles unmarked and tints spawned tiles by root task unless focused', () => {
    const root = info({ id: 's_root', name: 'lead', parent: null })
    const worker = info({ id: 's_worker', name: 'w1', parent: 's_root' })
    const leaf = info({ id: 's_leaf', name: 'leaf', parent: 's_worker' })
    const live = [root, worker, leaf]

    const plain = mountTile({ focused: false, sessionInfo: root, live })
    expect(plain.find('.root-chip').exists()).toBe(false)
    expect(plain.classes()).not.toContain('tinted')
    expect(plain.getComponent(SessionDot).props('tint')).toBeUndefined()

    const unfocusedChild = mountTile({ focused: false, sessionInfo: leaf, live })
    expect(unfocusedChild.get('.root-chip').text()).toBe('⟲ lead')
    expect(unfocusedChild.classes()).toContain('tinted')
    const dotTint = unfocusedChild.getComponent(SessionDot).props('tint')
    expect(dotTint).toMatch(/^var\(--agent-palette-\d+\)$/)
    expect(unfocusedChild.attributes('style')).toContain(`--root-tint: ${dotTint}`)

    const focusedChild = mountTile({ focused: true, sessionInfo: leaf, live })
    expect(focusedChild.get('.root-chip').text()).toBe('⟲ lead')
    expect(focusedChild.classes()).toContain('focused')
    expect(focusedChild.getComponent(SessionDot).props('tint')).toBeUndefined()
  })

  it('filters split view via header context menu', async () => {
    const root = info({ id: 's_root', name: 'lead', parent: null })
    const worker = info({ id: 's_worker', name: 'w1', parent: 's_root' })
    const wrapper = mountTile({ focused: false, sessionInfo: worker, live: [root, worker] })
    const workspace = useWorkspaceStore()

    await wrapper.get('header.header').trigger('contextmenu')
    const items = [...document.body.querySelectorAll('[role="menuitem"]')]
    expect(items.map((el) => el.textContent?.trim())).toEqual(['Created by lead', 'Spawned by w1'])
    ;(items[0] as HTMLElement).click()
    expect(workspace.splitFilter).toBe('s_root')
    wrapper.unmount()
    document.body.innerHTML = ''
  })
})
