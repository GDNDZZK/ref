import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import ImageLightbox from './ImageLightbox.vue'

describe('ImageLightbox', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('prevents wheel events on the lightbox scrim and image from scrolling behind it', () => {
    const wrapper = mount(ImageLightbox, {
      props: { url: 'https://example.invalid/pic.png', caption: 'pic' },
      attachTo: document.body,
      global: { plugins: [createAppI18n(['en'])] },
    })

    const scrim = document.body.querySelector('.scrim') as HTMLElement
    const picture = document.body.querySelector('.picture') as HTMLElement

    const scrimWheel = new WheelEvent('wheel', { deltaY: 80, bubbles: true, cancelable: true })
    scrim.dispatchEvent(scrimWheel)
    expect(scrimWheel.defaultPrevented).toBe(true)

    const pictureWheel = new WheelEvent('wheel', { deltaY: -80, bubbles: true, cancelable: true })
    picture.dispatchEvent(pictureWheel)
    expect(pictureWheel.defaultPrevented).toBe(true)

    wrapper.unmount()
  })
})
