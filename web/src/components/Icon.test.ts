import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { icons } from '../icons/generated/icons.ts'
import Icon from './Icon.vue'

describe('Icon', () => {
  it('draws the svg in the text color at its grid size', () => {
    const wrapper = mount(Icon, { props: { icon: 'copy-16' } })
    const svg = wrapper.get('svg')
    expect(svg.attributes('width')).toBe('16')
    expect(svg.attributes('stroke')).toBe('currentColor')
    expect(wrapper.classes()).toContain('size-16')
  })

  it('uses the 20 grid for -20 icons', () => {
    expect(mount(Icon, { props: { icon: 'plus-20' } }).classes()).toContain('size-20')
  })

  it('has only current-color icons whose size matches their name', () => {
    for (const [key, markup] of Object.entries(icons)) {
      const size = key.slice(-2)
      expect(markup, key).toContain(`width="${size}" height="${size}"`)
      expect(markup, key).not.toMatch(/#[0-9a-f]{3,6}\b|rgb\(/i)
    }
  })
})
