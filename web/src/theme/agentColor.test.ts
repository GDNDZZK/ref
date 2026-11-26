import { describe, expect, it } from 'vitest'
import {
  agentColor,
  agentColorByName,
  assignRootColors,
  paletteColorFor,
  ROOT_PALETTE,
} from './agentColor.ts'
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

describe('paletteColorFor', () => {
  it('returns a deterministic agent palette color for a session id', () => {
    const color = paletteColorFor('ses_root_1')
    expect(color).toMatch(/^var\(--agent-palette-\d+\)$/)
    expect(paletteColorFor('ses_root_1')).toBe(color)
  })
})

describe('assignRootColors', () => {
  it('assigns distinct colors to up to 7 colliding root ids and preserves older ids when newer ones arrive', () => {
    const collidingRoots = ['ses_0', 'ses_7', 'ses_E', 'ses_L', 'ses_S', 'ses_Z', 'ses_a']
    const initial = assignRootColors(['', ...collidingRoots, 'ses_0'])
    expect(initial.size).toBe(7)
    expect(new Set(initial.values()).size).toBe(7)
    for (const color of initial.values()) expect(ROOT_PALETTE).toContain(color)

    const expanded = assignRootColors([...collidingRoots, 'ses_h'])
    expect(expanded.size).toBe(8)
    for (const id of collidingRoots) expect(expanded.get(id)).toBe(initial.get(id))
    expect(ROOT_PALETTE).toContain(expanded.get('ses_h'))
  })
})
