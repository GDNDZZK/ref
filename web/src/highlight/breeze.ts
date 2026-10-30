// KSyntaxHighlighting's Breeze Light and Breeze Dark (data/themes/breeze-*.theme) as Shiki themes.
// Breeze colors text by its own style names (Keyword, ControlFlow, DataType, ...); each is
// mapped here to the TextMate scopes the grammars use for the same thing, so the same code looks
// alike in the GUI and the WebUI. Grammars differ from KSyntaxHighlighting's, so a token can land
// in a neighbouring style.

import type { ThemeRegistration } from 'shiki/core'

interface Palette {
  type: 'light' | 'dark'
  background: string
  normal: string
  keyword: string // bold
  functionName: string
  variable: string
  controlFlow: string // bold
  operator: string
  builtIn: string
  builtInBold: boolean
  attribute: string
  preprocessor: string
  char: string
  specialChar: string
  string: string
  specialString: string
  importName: string
  dataType: string
  number: string
  constant: string
  constantBold: boolean
  comment: string
  documentation: string
  annotation: string
  error: string
}

const LIGHT: Palette = {
  type: 'light',
  background: '#ffffff',
  normal: '#1f1c1b',
  keyword: '#1f1c1b',
  functionName: '#644a9b',
  variable: '#0057ae',
  controlFlow: '#1f1c1b',
  operator: '#ca60ca',
  builtIn: '#644a9b',
  builtInBold: true,
  attribute: '#0057ae',
  preprocessor: '#006e28',
  char: '#924c9d',
  specialChar: '#3daee9',
  string: '#bf0303',
  specialString: '#ff5500',
  importName: '#ff5500',
  dataType: '#0057ae',
  number: '#b08000',
  constant: '#aa5500',
  constantBold: false,
  comment: '#898887',
  documentation: '#607880',
  annotation: '#ca60ca',
  error: '#bf0303',
}

const DARK: Palette = {
  type: 'dark',
  background: '#232629',
  normal: '#cfcfc2',
  keyword: '#cfcfc2',
  functionName: '#8e44ad',
  variable: '#27aeae',
  controlFlow: '#fdbc4b',
  operator: '#3f8058',
  builtIn: '#609ca0',
  builtInBold: false,
  attribute: '#2980b9',
  preprocessor: '#27ae60',
  char: '#3daee9',
  specialChar: '#3daee9',
  string: '#f44f4f',
  specialString: '#da4453',
  importName: '#27ae60',
  dataType: '#2980b9',
  number: '#f67400',
  constant: '#27aeae',
  constantBold: true,
  comment: '#7a7c7d',
  documentation: '#a43340',
  annotation: '#3f8058',
  error: '#da4453',
}

const BOLD = 'bold'

function breeze(name: string, palette: Palette): ThemeRegistration {
  const rule = (scope: string[], foreground: string, fontStyle?: string) => ({
    scope,
    settings: fontStyle ? { foreground, fontStyle } : { foreground },
  })
  return {
    name,
    type: palette.type,
    colors: { 'editor.background': palette.background, 'editor.foreground': palette.normal },
    tokenColors: [
      rule(['comment', 'punctuation.definition.comment'], palette.comment),
      rule(
        ['comment.block.documentation', 'comment.block.javadoc', 'string.quoted.docstring'],
        palette.documentation,
      ),
      rule(['storage', 'keyword', 'entity.name.tag', 'markup.heading'], palette.keyword, BOLD),
      rule(['keyword.control'], palette.controlFlow, BOLD),
      rule(['keyword.operator', 'keyword.operator.assignment'], palette.operator),
      rule(
        [
          'entity.name.type',
          'entity.name.class',
          'support.type',
          'support.class',
          'keyword.type',
          'storage.type.primitive',
        ],
        palette.dataType,
      ),
      rule(['entity.name.function', 'support.function', 'meta.function-call'], palette.functionName),
      rule(
        ['support.function.builtin', 'support.type.builtin', 'variable.language', 'support.variable'],
        palette.builtIn,
        palette.builtInBold ? BOLD : undefined,
      ),
      rule(
        [
          'variable.other.normal',
          'variable.other.special',
          'variable.other.bracket',
          'punctuation.definition.variable',
          'variable.parameter.positional',
        ],
        palette.variable,
      ),
      rule(
        ['constant.language', 'constant.other', 'variable.other.constant', 'support.constant'],
        palette.constant,
        palette.constantBold ? BOLD : undefined,
      ),
      rule(['constant.numeric'], palette.number),
      rule(['constant.character'], palette.char),
      rule(['constant.character.escape', 'constant.other.placeholder'], palette.specialChar),
      rule(['string'], palette.string),
      rule(['string.regexp'], palette.specialString),
      rule(
        [
          'meta.preprocessor',
          'keyword.control.directive',
          'keyword.other.preprocessor',
          'entity.name.function.preprocessor',
        ],
        palette.preprocessor,
      ),
      rule(
        [
          'keyword.control.import',
          'keyword.control.from',
          'keyword.control.export',
          'keyword.other.use',
          'string.quoted.other.lt-gt.include',
        ],
        palette.importName,
      ),
      rule(
        [
          'entity.other.attribute-name',
          'meta.attribute',
          'support.type.property-name',
          'meta.object-literal.key',
        ],
        palette.attribute,
      ),
      rule(
        [
          'storage.type.annotation',
          'meta.annotation',
          'meta.decorator',
          'entity.name.function.decorator',
          'punctuation.definition.annotation',
        ],
        palette.annotation,
      ),
      rule(['markup.inserted'], palette.preprocessor),
      rule(['markup.deleted'], palette.string),
      rule(['markup.bold'], palette.normal, BOLD),
      rule(['markup.italic'], palette.normal, 'italic'),
      rule(['markup.inline.raw', 'markup.fenced_code'], palette.string),
      rule(['invalid'], palette.error, 'underline'),
    ],
  }
}

export const BREEZE_LIGHT = breeze('breeze-light', LIGHT)
export const BREEZE_DARK = breeze('breeze-dark', DARK)
