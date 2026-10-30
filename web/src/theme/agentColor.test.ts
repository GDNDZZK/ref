import { describe, expect, it } from 'vitest'
import { agentColor, agentColorByName } from './agentColor.ts'
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

describe('agentColorByName', () => {
  const agents = [
    { name: 'build', color: null },
    { name: 'plan', color: 'blue' },
  ]

  it('uses the configured color, else the position in the list', () => {
    expect(agentColorByName(agents, 'plan')).toBe('var(--agent-blue)')
    expect(agentColorByName(agents, 'build')).toBe('var(--agent-palette-0)')
  })

  it('hashes the name of an agent the list does not have', () => {
    const color = agentColorByName(agents, 'ghost')
    expect(color).toMatch(/^var\(--agent-palette-\d\)$/)
    expect(agentColorByName(agents, 'ghost')).toBe(color)
  })
})
