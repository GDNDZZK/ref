import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../../i18n/index.ts'
import { RefError } from '../../net/connection.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import PathDialog from './PathDialog.vue'

describe('PathDialog', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  function setup(listDirs: (path: string) => Promise<{ path: string; dirs: string[] }>) {
    const workspace = useWorkspaceStore()
    const notices = useNoticesStore()
    workspace.cwd = '/work/'
    workspace.listDirs = vi.fn(listDirs)
    const wrapper = mount(PathDialog, {
      props: { mode: 'newSession' },
      global: { plugins: [createAppI18n(['en'])] },
    })
    return { wrapper, workspace, notices }
  }

  it('treats coded not_found and dir_not_found silently as an empty completion list', async () => {
    for (const code of ['not_found', 'dir_not_found']) {
      setActivePinia(createPinia())
      const { wrapper, notices } = setup(() => Promise.reject(new RefError('No such folder on disk', code)))
      await vi.advanceTimersByTimeAsync(200)
      expect(notices.notices).toEqual([])
      expect(wrapper.findAll('.entry')).toHaveLength(0)
      expect(wrapper.text()).toContain('No sub-directories')
    }
  })

  it('treats 0.2.0 missing-directory text fallback silently as an empty list', async () => {
    for (const message of ['Directory does not exist: /', 'Not a directory: /']) {
      setActivePinia(createPinia())
      const { wrapper, notices } = setup(() => Promise.reject(new Error(message)))
      await vi.advanceTimersByTimeAsync(200)

      expect(notices.notices).toEqual([])
      expect(wrapper.findAll('.entry')).toHaveLength(0)
    }
  })

  it('reports non-missing-directory errors via notices.pushError', async () => {
    const { wrapper, notices } = setup(() => Promise.reject(new RefError('Permission denied')))
    await vi.advanceTimersByTimeAsync(200)

    expect(wrapper.findAll('.entry')).toHaveLength(0)
    expect(notices.notices).toHaveLength(1)
    expect(notices.notices[0]).toMatchObject({ level: 'error', text: 'Permission denied' })
  })
})
