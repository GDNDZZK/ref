// Shared helpers for lossless graphical editing of `ref.json` sections.

import type { SelectOption } from '../components/config/SelectField.vue'
import type { AgentInfo, ModelInfo } from '../proto/generated'
import {
  BUILTIN_AGENTS,
  cloneJson,
  getAt,
  getObjectAt,
  getStringArrayAt,
  isPlainObject,
  setAt,
  setOwn,
  type JsonObject,
} from './jsonPatch.ts'

export function renameObjectKey(source: JsonObject, oldKey: string, newKey: string): JsonObject {
  const trimmed = newKey.trim()
  if (trimmed === '' || trimmed === oldKey || Object.hasOwn(source, trimmed)) return cloneJson(source)
  const out: JsonObject = {}
  for (const [key, value] of Object.entries(source)) {
    setOwn(out, key === oldKey ? trimmed : key, cloneJson(value))
  }
  return out
}

export function uniqueObjectKey(source: JsonObject | undefined, baseName: string): string {
  const base = baseName.trim() || 'item'
  if (!source || !Object.hasOwn(source, base)) return base
  let suffix = 2
  while (Object.hasOwn(source, `${base}-${suffix}`)) {
    suffix += 1
  }
  return `${base}-${suffix}`
}

export function configuredModelOptions(
  doc: JsonObject,
  serverModels: readonly ModelInfo[] = [],
): SelectOption[] {
  const seen = new Map<string, string>()

  const providers = getObjectAt(doc, ['provider'])
  if (providers) {
    for (const [providerId, rawProvider] of Object.entries(providers)) {
      if (!isPlainObject(rawProvider)) continue
      const models = getObjectAt(rawProvider, ['models'])
      if (!models) continue
      for (const [modelKey, rawModel] of Object.entries(models)) {
        const ref = `${providerId}/${modelKey}`
        const displayName =
          isPlainObject(rawModel) && typeof rawModel.name === 'string' && rawModel.name.trim() !== ''
            ? `${rawModel.name} (${ref})`
            : ref
        seen.set(ref, displayName)
      }
    }
  }

  for (const model of serverModels) {
    const ref = `${model.provider}/${model.id}`
    if (!seen.has(ref)) {
      const label = model.name && model.name !== model.id ? `${model.name} (${ref})` : ref
      seen.set(ref, label)
    }
  }

  return [...seen.entries()].map(([value, label]) => ({ value, label }))
}

export function configuredAgentOptions(
  doc: JsonObject,
  serverAgents: readonly AgentInfo[] = [],
  includeCompaction = false,
): SelectOption[] {
  const names = new Set<string>()
  for (const name of BUILTIN_AGENTS) {
    if (!includeCompaction && name === 'compaction') continue
    names.add(name)
  }
  const docAgents = getObjectAt(doc, ['agent'])
  if (docAgents) {
    for (const [name, entry] of Object.entries(docAgents)) {
      if (isPlainObject(entry) && (entry.disable === true || entry.disabled === true)) {
        names.delete(name)
        continue
      }
      if (!includeCompaction && name === 'compaction') continue
      names.add(name)
    }
  }
  for (const agent of serverAgents) {
    if (!includeCompaction && agent.name === 'compaction') continue
    names.add(agent.name)
  }
  return [...names].map((value) => ({ value, label: value }))
}

// Splits a shell command line into argv tokens, honoring single quotes, double quotes, and `\` escapes.
export function splitShellWords(line: string): string[] {
  const tokens: string[] = []
  let current = ''
  let hasToken = false
  let quote: "'" | '"' | null = null
  const chars = Array.from(line)

  for (let index = 0; index < chars.length; index += 1) {
    const ch = chars[index]!
    if (quote === "'") {
      if (ch === "'") quote = null
      else current += ch
      continue
    }
    if (quote === '"') {
      if (ch === '"') {
        quote = null
        continue
      }
      if (ch === '\\' && index + 1 < chars.length) {
        index += 1
        current += chars[index]!
        continue
      }
      current += ch
      continue
    }
    if (ch === "'" || ch === '"') {
      quote = ch
      hasToken = true
      continue
    }
    if (ch === '\\' && index + 1 < chars.length) {
      index += 1
      current += chars[index]!
      hasToken = true
      continue
    }
    if (/\s/.test(ch)) {
      if (hasToken) {
        tokens.push(current)
        current = ''
        hasToken = false
      }
      continue
    }
    current += ch
    hasToken = true
  }

  if (hasToken) tokens.push(current)
  return tokens
}

// Reads `skills` whether stored as `["..."]`, `"..."`, or `{ "paths": ["..."] }`.
export function parseSkillsPaths(doc: JsonObject): string[] {
  const raw = getAt(doc, ['skills'])
  if (typeof raw === 'string') return raw.trim() ? [raw] : []
  if (Array.isArray(raw)) return getStringArrayAt(doc, ['skills'])
  if (isPlainObject(raw)) {
    const paths = raw.paths
    if (typeof paths === 'string') return paths.trim() ? [paths] : []
    return getStringArrayAt(doc, ['skills', 'paths'])
  }
  return []
}

// Writes `skills` losslessly, preserving array form if originally an array, else `{ paths: [...] }`.
export function patchSkillsPaths(doc: JsonObject, nextPaths: readonly string[]): JsonObject {
  const cleaned = nextPaths.filter((item) => item.trim() !== '')
  const raw = getAt(doc, ['skills'])
  if (Array.isArray(raw)) {
    return setAt(doc, ['skills'], cleaned.length > 0 ? cleaned : undefined)
  }
  return setAt(doc, ['skills', 'paths'], cleaned.length > 0 ? cleaned : undefined)
}

// Preserves trailing spaces while typing prose, collapsing blank strings to `undefined`.
export function normalizeProseInput(value: string): string | undefined {
  return value.trim() === '' ? undefined : value
}

// Trims leading/trailing whitespace on blur or identifier commit, collapsing blank strings to `undefined`.
export function normalizeTrimmedString(value: string): string | undefined {
  const trimmed = value.trim()
  return trimmed === '' ? undefined : trimmed
}

export const MANAGED_PROVIDER_OPTION_KEYS: ReadonlySet<string> = new Set([
  'baseURL',
  'baseUrl',
  'base_url',
  'apiKey',
  'api_key',
  'setCacheKey',
  'chunkTimeout',
  'headers',
])

export function extractExtraProviderOptions(provider: JsonObject): JsonObject | undefined {
  const options = getObjectAt(provider, ['options'])
  if (!options) return undefined
  const extra: JsonObject = {}
  let count = 0
  for (const [key, value] of Object.entries(options)) {
    if (MANAGED_PROVIDER_OPTION_KEYS.has(key)) continue
    setOwn(extra, key, cloneJson(value))
    count += 1
  }
  return count > 0 ? extra : undefined
}

// Merges user-edited extra `provider.<id>.options` keys with dedicated managed keys without erasing
// or reordering existing managed keys (`apiKey`, `baseURL`, `setCacheKey`, `chunkTimeout`, `headers`).
export function mergeExtraProviderOptions(
  provider: JsonObject,
  extraOptions: JsonObject | undefined,
): JsonObject | undefined {
  const existing = getObjectAt(provider, ['options']) ?? {}
  const merged: JsonObject = {}
  const writtenExtra = new Set<string>()

  for (const [key, value] of Object.entries(existing)) {
    if (MANAGED_PROVIDER_OPTION_KEYS.has(key)) {
      setOwn(merged, key, cloneJson(value))
      continue
    }
    if (extraOptions && Object.hasOwn(extraOptions, key)) {
      setOwn(merged, key, cloneJson(extraOptions[key]))
      writtenExtra.add(key)
    }
  }

  if (extraOptions) {
    for (const [key, value] of Object.entries(extraOptions)) {
      if (MANAGED_PROVIDER_OPTION_KEYS.has(key) || writtenExtra.has(key)) continue
      setOwn(merged, key, cloneJson(value))
    }
  }

  return Object.keys(merged).length > 0 ? merged : undefined
}
