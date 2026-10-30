// The languages that get highlighting. Each grammar is its own chunk, fetched the first time a
// code block uses it. Not the whole Shiki bundle: it has hundreds of grammars.

import type { LanguageInput } from 'shiki/core'

type Loader = () => Promise<LanguageInput>

const LOADERS = {
  bash: () => import('shiki/langs/bash.mjs'),
  c: () => import('shiki/langs/c.mjs'),
  cmake: () => import('shiki/langs/cmake.mjs'),
  cpp: () => import('shiki/langs/cpp.mjs'),
  csharp: () => import('shiki/langs/csharp.mjs'),
  css: () => import('shiki/langs/css.mjs'),
  diff: () => import('shiki/langs/diff.mjs'),
  dockerfile: () => import('shiki/langs/dockerfile.mjs'),
  go: () => import('shiki/langs/go.mjs'),
  haskell: () => import('shiki/langs/haskell.mjs'),
  html: () => import('shiki/langs/html.mjs'),
  ini: () => import('shiki/langs/ini.mjs'),
  java: () => import('shiki/langs/java.mjs'),
  javascript: () => import('shiki/langs/javascript.mjs'),
  json: () => import('shiki/langs/json.mjs'),
  jsonc: () => import('shiki/langs/jsonc.mjs'),
  jsx: () => import('shiki/langs/jsx.mjs'),
  kotlin: () => import('shiki/langs/kotlin.mjs'),
  lua: () => import('shiki/langs/lua.mjs'),
  makefile: () => import('shiki/langs/makefile.mjs'),
  markdown: () => import('shiki/langs/markdown.mjs'),
  perl: () => import('shiki/langs/perl.mjs'),
  php: () => import('shiki/langs/php.mjs'),
  powershell: () => import('shiki/langs/powershell.mjs'),
  python: () => import('shiki/langs/python.mjs'),
  ruby: () => import('shiki/langs/ruby.mjs'),
  rust: () => import('shiki/langs/rust.mjs'),
  scala: () => import('shiki/langs/scala.mjs'),
  scss: () => import('shiki/langs/scss.mjs'),
  sql: () => import('shiki/langs/sql.mjs'),
  swift: () => import('shiki/langs/swift.mjs'),
  toml: () => import('shiki/langs/toml.mjs'),
  tsx: () => import('shiki/langs/tsx.mjs'),
  typescript: () => import('shiki/langs/typescript.mjs'),
  xml: () => import('shiki/langs/xml.mjs'),
  yaml: () => import('shiki/langs/yaml.mjs'),
} as const satisfies Record<string, Loader>

export type LanguageId = keyof typeof LOADERS

// Info strings that code fences use, and file extensions, for the languages above.
const ALIASES: Record<string, LanguageId> = {
  sh: 'bash',
  shell: 'bash',
  shellscript: 'bash',
  zsh: 'bash',
  console: 'bash',
  'c++': 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  h: 'c',
  hpp: 'cpp',
  cs: 'csharp',
  'c#': 'csharp',
  patch: 'diff',
  docker: 'dockerfile',
  golang: 'go',
  hs: 'haskell',
  htm: 'html',
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  json5: 'jsonc',
  kt: 'kotlin',
  make: 'makefile',
  md: 'markdown',
  pl: 'perl',
  ps1: 'powershell',
  pwsh: 'powershell',
  py: 'python',
  rb: 'ruby',
  rs: 'rust',
  ts: 'typescript',
  mts: 'typescript',
  svg: 'xml',
  yml: 'yaml',
  cfg: 'ini',
}

export function languageFromName(name: string): LanguageId | undefined {
  const key = name.trim().toLowerCase()
  if (key in LOADERS) return key as LanguageId
  return ALIASES[key]
}

// The language of a file, from its extension.
export function languageFromPath(path: string): LanguageId | undefined {
  const base = path.slice(path.lastIndexOf('/') + 1).toLowerCase()
  if (base === 'dockerfile' || base === 'makefile' || base === 'cmakelists.txt') {
    return languageFromName(base === 'cmakelists.txt' ? 'cmake' : base)
  }
  const dot = base.lastIndexOf('.')
  return dot < 0 ? undefined : languageFromName(base.slice(dot + 1))
}

export function loadLanguage(id: LanguageId): Promise<LanguageInput> {
  return LOADERS[id]()
}
