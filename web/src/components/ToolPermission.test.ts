import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { describe, expect, it } from 'vitest'
import permissionFixture from '../../../crates/proto/fixtures/session_event/permission.json'
import { createAppI18n } from '../i18n/index.ts'
import type { PermissionRequest } from '../proto/generated'
import ToolPermission from './ToolPermission.vue'

const BASE_REQUEST = permissionFixture.request as PermissionRequest

function mountPermission(request: PermissionRequest | undefined) {
  return mount(ToolPermission, {
    props: { request, waiting: 0 },
    global: { plugins: [createPinia(), createAppI18n(['en'])] },
  })
}

describe('ToolPermission', () => {
  it('renders the preview diff block above the buttons when preview is present', () => {
    const wrapper = mountPermission(BASE_REQUEST)

    expect(wrapper.text()).toContain('Preview')
    expect(wrapper.get('.added').text()).toBe('+1')
    expect(wrapper.get('.removed').text()).toBe('-1')
    expect(wrapper.find('.rows').exists()).toBe(true)
    expect(wrapper.find('.chip').exists()).toBe(false)

    const previewEl = wrapper.get('.rows').element
    const choicesEl = wrapper.get('.choices').element
    expect(previewEl.compareDocumentPosition(choicesEl) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('omits the preview block when preview is absent', () => {
    const wrapper = mountPermission({ ...BASE_REQUEST, preview: undefined })

    expect(wrapper.text()).not.toContain('Preview')
    expect(wrapper.find('.rows').exists()).toBe(false)
    expect(wrapper.find('.added').exists()).toBe(false)
    expect(wrapper.find('.removed').exists()).toBe(false)
  })

  it('shows the truncated marker when preview.truncated is true', () => {
    const wrapper = mountPermission({
      ...BASE_REQUEST,
      preview: {
        ...BASE_REQUEST.preview!,
        added: 20,
        removed: 5,
        truncated: true,
      },
    })

    expect(wrapper.get('.added').text()).toBe('+20')
    expect(wrapper.get('.removed').text()).toBe('-5')
    expect(wrapper.get('.chip').text()).toBe('truncated')
  })
})
