// Reads `property` declarations whose values are literals out of QML files.
//
//   readonly property color accent: dark ? "#7aa2f7" : "#94caef"   -> themed pair
//   readonly property int quick: 120                                -> number
//   readonly property int ease: Easing.OutExpo                      -> easing
//   readonly property string syntaxTheme: dark ? "Breeze Dark" : "Breeze Light"
//
// Anything else is an error: a value that depends on other properties cannot be generated.

export type Literal = number | string

export type Value =
  | { kind: 'number'; value: number }
  | { kind: 'string'; value: string }
  | { kind: 'easing'; name: string }
  | { kind: 'themed'; dark: Literal; light: Literal }

export interface QmlProperty {
  name: string
  type: string
  readonly: boolean
  expression: string
  line: number
}

const DECLARATION = /^\s*(readonly\s+)?property\s+(\w+)\s+(\w+)\s*:\s*(.*)$/
const NUMBER = /^-?\d+(?:\.\d+)?$/
const STRING = /^"([^"\\]*)"$/
const EASING = /^Easing\.(\w+)$/
const THEMED_PREFIX = /^dark\s*\?\s*/

// Removes a trailing `// comment` that is not inside a string.
function stripComment(text: string): string {
  let inString = false
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (c === '"') inString = !inString
    if (!inString && c === '/' && text[i + 1] === '/') return text.slice(0, i).trim()
  }
  return text.trim()
}

export function parseProperties(source: string): QmlProperty[] {
  const properties: QmlProperty[] = []
  source.split('\n').forEach((text, index) => {
    const match = DECLARATION.exec(text)
    if (!match) return
    properties.push({
      name: match[3]!,
      type: match[2]!,
      readonly: match[1] !== undefined,
      expression: stripComment(match[4]!),
      line: index + 1,
    })
  })
  return properties
}

function parseLiteral(text: string): Literal | undefined {
  const trimmed = text.trim()
  if (NUMBER.test(trimmed)) return Number(trimmed)
  const string = STRING.exec(trimmed)
  return string ? string[1]! : undefined
}

// Throws unless `property` is one of the supported literal forms.
export function parseValue(property: QmlProperty, where: string): Value {
  const fail = (): never => {
    throw new Error(
      `${where}:${property.line}: cannot generate "${property.name}": unsupported value \`${property.expression}\``,
    )
  }

  const { expression } = property
  const plain = parseLiteral(expression)
  if (typeof plain === 'number') return { kind: 'number', value: plain }
  if (typeof plain === 'string') return { kind: 'string', value: plain }

  const easing = EASING.exec(expression)
  if (easing) return { kind: 'easing', name: easing[1]! }

  if (!THEMED_PREFIX.test(expression)) return fail()
  const branches = expression.replace(THEMED_PREFIX, '').split(/\s:\s/)
  if (branches.length !== 2) return fail()
  const dark = parseLiteral(branches[0]!)
  const light = parseLiteral(branches[1]!)
  if (dark === undefined || light === undefined) return fail()
  return { kind: 'themed', dark, light }
}

export function findProperty(properties: readonly QmlProperty[], name: string, where: string): QmlProperty {
  const found = properties.filter((p) => p.name === name)
  if (found.length === 0) throw new Error(`${where}: property "${name}" not found`)
  if (found.length > 1) throw new Error(`${where}: property "${name}" is declared more than once`)
  return found[0]!
}
