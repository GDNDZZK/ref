import { describe, expect, it } from 'vitest'
import { findProperty, parseProperties, parseValue } from './qmlProperties.ts'

function valueOf(declaration: string) {
  const [property] = parseProperties(declaration)
  return parseValue(property!, 'test.qml')
}

describe('parseValue', () => {
  it('reads themed colors', () => {
    expect(valueOf('\treadonly property color accent: dark ? "#7aa2f7" : "#94caef"')).toEqual({
      kind: 'themed',
      dark: '#7aa2f7',
      light: '#94caef',
    })
  })

  it('reads numbers, strings and easings, ignoring trailing comments', () => {
    expect(valueOf('readonly property int quick: 120    // hover, press')).toEqual({
      kind: 'number',
      value: 120,
    })
    expect(valueOf('readonly property real scale: 0.35')).toEqual({ kind: 'number', value: 0.35 })
    expect(valueOf('readonly property string s: "a // b"')).toEqual({ kind: 'string', value: 'a // b' })
    expect(valueOf('readonly property int ease: Easing.OutExpo')).toEqual({ kind: 'easing', name: 'OutExpo' })
  })

  it('reads themed numbers and strings', () => {
    expect(valueOf('readonly property string t: dark ? "Breeze Dark" : "Breeze Light"')).toEqual({
      kind: 'themed',
      dark: 'Breeze Dark',
      light: 'Breeze Light',
    })
  })

  it.each([
    'readonly property real halfSpan: Math.min(width, height) * 0.3',
    'readonly property int period: Motion.pulse',
    'readonly property color stateColor: running ? Theme.accent : failed ? Theme.danger : Theme.success',
  ])('rejects %s with its location', (declaration) => {
    expect(() => valueOf(declaration)).toThrow(/test\.qml:1: cannot generate/)
  })
})

describe('lists of property names', () => {
  it('reads an array of other properties', () => {
    expect(valueOf('readonly property var agentPalette: [agentBlue, agentPurple,agentGreen]')).toEqual({
      kind: 'list',
      names: ['agentBlue', 'agentPurple', 'agentGreen'],
    })
  })

  it.each(['[]', '[1, 2]', '[a, "b"]', '[a, b.c]'])('rejects %s', (list) => {
    expect(() => valueOf(`readonly property var x: ${list}`)).toThrow(/cannot generate/)
  })
})

describe('findProperty', () => {
  const properties = parseProperties('property int a: 1\nproperty int b: 2\nproperty int b: 3\n')

  it('finds a unique property', () => {
    expect(findProperty(properties, 'a', 'x.qml').expression).toBe('1')
  })

  it('reports missing and duplicate properties', () => {
    expect(() => findProperty(properties, 'c', 'x.qml')).toThrow('not found')
    expect(() => findProperty(properties, 'b', 'x.qml')).toThrow('more than once')
  })
})
