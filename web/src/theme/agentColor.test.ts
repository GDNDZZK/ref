import { describe, expect, it } from 'vitest'
import { agentColor } from './agentColor.ts'
import { agentPalette, colors } from './generated/theme.ts'

describe('agentColor', () => {
  it('maps the names of the TUI configuration to the agent colors', () => {
    expect(agentColor('primary', 0)).toBe('var(--agent-orange)')
    expect(agentColor('Secondary', 0)).toBe('var(--agent-blue)')
    expect(agentColor(' success ', 0)).toBe('var(--agent-green)')
    expect(agentColor('info', 0)).toBe('var(--agent-cyan)')
  })

  it('keeps hex colors as written', () => {
    expect(agentColor('#A1b2c3', 3)).toBe('#a1b2c3')
    expect(agentColor('#fff', 3)).toBe('#fff')
  })

  it('falls back to the palette by position, wrapping around', () => {
    const size = agentPalette.length
    expect(agentColor('', 0)).toBe('var(--agent-palette-0)')
    expect(agentColor('chartreuse', 1)).toBe('var(--agent-palette-1)')
    expect(agentColor('#12', size)).toBe('var(--agent-palette-0)')
    expect(agentColor('', -1)).toBe('var(--agent-palette-1)')
  })

  it('does not take object prototype names for colors', () => {
    expect(agentColor('constructor', 0)).toBe('var(--agent-palette-0)')
  })

  it('lists palette entries that exist as colors', () => {
    for (const name of agentPalette) expect(colors.light).toHaveProperty(name)
  })
})
