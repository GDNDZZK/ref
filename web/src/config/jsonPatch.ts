// Lossless plain-object JSON patch helpers, provider/agent catalogs, and permission rule
// codecs for the graphical ref.json editor. Untouched keys, key order, and `{env:...}` /
// `{file:...}` placeholders remain intact across edits.

export type JsonObject = Record<string, unknown>

const JSON_INDENT_SPACES = 2

const ENV_REF_GLOBAL = /\{env:\s*([^}\s]+)\s*\}/g
const SECRET_PLACEHOLDER = /\{(?:env:\s*[^}\s]+\s*|file:\s*[^}]*\S[^}]*)\}/

const WILDCARD = '*'

export function isPlainObject(value: unknown): value is JsonObject {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false
  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}

// Assigns an own data property without triggering `__proto__` setter semantics.
export function setOwn<T>(target: Record<string, T>, key: string, value: NoInfer<T>): void {
  Object.defineProperty(target, key, {
    value,
    writable: true,
    enumerable: true,
    configurable: true,
  })
}

export function cloneJson<T>(value: T): T {
  if (Array.isArray(value)) return value.map((item) => cloneJson(item)) as T
  if (!isPlainObject(value)) return value
  const copy: JsonObject = {}
  for (const [key, child] of Object.entries(value)) {
    setOwn(copy, key, cloneJson(child))
  }
  return copy as T
}

export function formatJson(root: JsonObject): string {
  return `${JSON.stringify(root, null, JSON_INDENT_SPACES)}\n`
}

export function getAt(root: unknown, path: readonly string[]): unknown {
  let current: unknown = root
  for (const segment of path) {
    if (!isPlainObject(current) || !Object.hasOwn(current, segment)) return undefined
    current = current[segment]
  }
  return current
}

export function getStringAt(root: unknown, path: readonly string[]): string | undefined {
  const value = getAt(root, path)
  return typeof value === 'string' ? value : undefined
}

export function getNumberAt(root: unknown, path: readonly string[]): number | undefined {
  const value = getAt(root, path)
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

export function getBooleanAt(root: unknown, path: readonly string[]): boolean | undefined {
  const value = getAt(root, path)
  return typeof value === 'boolean' ? value : undefined
}

export function getObjectAt(root: unknown, path: readonly string[]): JsonObject | undefined {
  const value = getAt(root, path)
  return isPlainObject(value) ? value : undefined
}

export function getStringArrayAt(root: unknown, path: readonly string[]): string[] {
  const value = getAt(root, path)
  if (!Array.isArray(value)) return []
  return value.filter((item): item is string => typeof item === 'string')
}

export function getStringMapAt(root: unknown, path: readonly string[]): Record<string, string> {
  const value = getAt(root, path)
  if (!isPlainObject(value)) return {}
  const out: Record<string, string> = {}
  for (const [key, entry] of Object.entries(value)) {
    if (typeof entry === 'string') setOwn(out, key, entry)
  }
  return out
}

function copyWithoutKey(source: JsonObject, removedKey: string): JsonObject {
  const out: JsonObject = {}
  for (const [key, entry] of Object.entries(source)) {
    if (key !== removedKey) setOwn(out, key, cloneJson(entry))
  }
  return out
}

function copyWithKey(source: JsonObject, targetKey: string, nextValue: unknown): JsonObject {
  const out: JsonObject = {}
  let replaced = false
  for (const [key, entry] of Object.entries(source)) {
    if (key === targetKey) {
      setOwn(out, key, nextValue)
      replaced = true
      continue
    }
    setOwn(out, key, cloneJson(entry))
  }
  if (!replaced) setOwn(out, targetKey, nextValue)
  return out
}

interface PatchStep {
  next: JsonObject
  changed: boolean
  emptied: boolean
}

function patchObject(current: JsonObject, path: readonly string[], index: number, value: unknown): PatchStep {
  const segment = path[index]!
  const isLeaf = index === path.length - 1
  const hadKey = Object.hasOwn(current, segment)

  if (isLeaf) {
    if (value === undefined) {
      if (!hadKey) return { next: cloneJson(current), changed: false, emptied: false }
      const next = copyWithoutKey(current, segment)
      return { next, changed: true, emptied: Object.keys(next).length === 0 }
    }
    const next = copyWithKey(current, segment, cloneJson(value))
    return { next, changed: true, emptied: false }
  }

  const existing = hadKey ? current[segment] : undefined
  if (value === undefined && !isPlainObject(existing)) {
    return { next: cloneJson(current), changed: false, emptied: false }
  }

  const childSource = isPlainObject(existing) ? existing : {}
  const childStep = patchObject(childSource, path, index + 1, value)
  if (!childStep.changed) return { next: cloneJson(current), changed: false, emptied: false }

  if (childStep.emptied) {
    const next = copyWithoutKey(current, segment)
    return { next, changed: true, emptied: Object.keys(next).length === 0 }
  }

  const next = copyWithKey(current, segment, childStep.next)
  return { next, changed: true, emptied: false }
}

// Immutably sets `path` to `value` on `root`. Setting `undefined` deletes the leaf key and
// prunes any intermediate objects along `path` that became empty from that deletion, while
// leaving untouched sibling keys (even empty ones) and key insertion order intact.
export function setAt(root: JsonObject, path: readonly string[], value: unknown): JsonObject {
  if (path.length === 0) return isPlainObject(value) ? cloneJson(value) : cloneJson(root)
  return patchObject(root, path, 0, value).next
}

function collectEnvRefs(value: unknown, into: Set<string>): void {
  if (typeof value === 'string') {
    for (const match of value.matchAll(ENV_REF_GLOBAL)) {
      if (match[1]) into.add(match[1])
    }
    return
  }
  if (Array.isArray(value)) {
    for (const item of value) collectEnvRefs(item, into)
    return
  }
  if (isPlainObject(value)) {
    for (const entry of Object.values(value)) collectEnvRefs(entry, into)
  }
}

export function extractEnvRefs(value: unknown): string[] {
  const found = new Set<string>()
  collectEnvRefs(value, found)
  return [...found].sort()
}

export function isSecretPlaceholder(value: string): boolean {
  return SECRET_PLACEHOLDER.test(value)
}

export const BUILTIN_AGENTS = ['build', 'plan', 'general', 'explore', 'compaction'] as const
export type BuiltinAgentName = (typeof BUILTIN_AGENTS)[number]

export interface BuiltinAgentMeta {
  name: BuiltinAgentName
  mode: 'primary' | 'subagent'
  hidden: boolean
  canSpawnDefault: boolean
}

export const BUILTIN_AGENT_META: readonly BuiltinAgentMeta[] = [
  { name: 'build', mode: 'primary', hidden: false, canSpawnDefault: true },
  { name: 'plan', mode: 'primary', hidden: false, canSpawnDefault: false },
  { name: 'general', mode: 'subagent', hidden: false, canSpawnDefault: false },
  { name: 'explore', mode: 'subagent', hidden: false, canSpawnDefault: false },
  { name: 'compaction', mode: 'primary', hidden: true, canSpawnDefault: false },
]

export type ProviderPresetId = 'anthropic' | 'openai' | 'deepseek' | 'openrouter' | 'ollama' | 'custom'

export type ProviderNpmPackage = '@ai-sdk/anthropic' | '@ai-sdk/openai' | '@ai-sdk/openai-compatible'

export type ProviderApiKind = 'anthropic' | 'responses' | 'chat'

export interface ProviderPreset {
  id: ProviderPresetId
  providerId: string
  label: string
  npm: ProviderNpmPackage
  api: ProviderApiKind | undefined
  defaultBaseUrl: string
  envVar: string
  apiKeyPlaceholder: string
  suggestedModels: readonly string[]
}

export const PROVIDER_PRESETS: readonly ProviderPreset[] = [
  {
    id: 'anthropic',
    providerId: 'anthropic',
    label: 'Anthropic',
    npm: '@ai-sdk/anthropic',
    api: 'anthropic',
    defaultBaseUrl: 'https://api.anthropic.com/v1',
    envVar: 'ANTHROPIC_API_KEY',
    apiKeyPlaceholder: '{env:ANTHROPIC_API_KEY}',
    suggestedModels: ['claude-sonnet-4-5', 'claude-opus-4-1', 'claude-haiku-4-5'],
  },
  {
    id: 'openai',
    providerId: 'openai',
    label: 'OpenAI',
    npm: '@ai-sdk/openai',
    api: 'responses',
    defaultBaseUrl: 'https://api.openai.com/v1',
    envVar: 'OPENAI_API_KEY',
    apiKeyPlaceholder: '{env:OPENAI_API_KEY}',
    suggestedModels: ['gpt-5', 'gpt-5-mini'],
  },
  {
    id: 'deepseek',
    providerId: 'deepseek',
    label: 'DeepSeek',
    npm: '@ai-sdk/openai-compatible',
    api: 'chat',
    defaultBaseUrl: 'https://api.deepseek.com/v1',
    envVar: 'DEEPSEEK_API_KEY',
    apiKeyPlaceholder: '{env:DEEPSEEK_API_KEY}',
    suggestedModels: ['deepseek-chat', 'deepseek-reasoner'],
  },
  {
    id: 'openrouter',
    providerId: 'openrouter',
    label: 'OpenRouter',
    npm: '@ai-sdk/openai-compatible',
    api: 'chat',
    defaultBaseUrl: 'https://openrouter.ai/api/v1',
    envVar: 'OPENROUTER_API_KEY',
    apiKeyPlaceholder: '{env:OPENROUTER_API_KEY}',
    suggestedModels: ['anthropic/claude-sonnet-4.5', 'openai/gpt-5'],
  },
  {
    id: 'ollama',
    providerId: 'ollama',
    label: 'Ollama / vLLM',
    npm: '@ai-sdk/openai-compatible',
    api: 'chat',
    defaultBaseUrl: 'http://localhost:11434/v1',
    envVar: '',
    apiKeyPlaceholder: '',
    suggestedModels: [],
  },
  {
    id: 'custom',
    providerId: '',
    label: 'Custom',
    npm: '@ai-sdk/openai-compatible',
    api: undefined,
    defaultBaseUrl: 'https://example.com/v1',
    envVar: 'MY_API_KEY',
    apiKeyPlaceholder: '{env:MY_API_KEY}',
    suggestedModels: [],
  },
]

export const PERMISSION_ACTIONS = ['allow', 'ask', 'deny'] as const
export type PermissionAction = (typeof PERMISSION_ACTIONS)[number]

export interface PermissionRule {
  tool: string
  pattern: string
  action: PermissionAction
}

export interface ParsedPermissions {
  mode: 'global' | 'rules'
  globalAction: PermissionAction
  rules: PermissionRule[]
}

const ACTION_ALIASES: Readonly<Record<string, PermissionAction>> = {
  allow: 'allow',
  auto: 'allow',
  yes: 'allow',
  true: 'allow',
  ask: 'ask',
  confirm: 'ask',
  prompt: 'ask',
  deny: 'deny',
  reject: 'deny',
  no: 'deny',
  false: 'deny',
}

export function normalizePermissionAction(value: unknown): PermissionAction | undefined {
  if (typeof value === 'boolean') return value ? 'allow' : 'deny'
  if (typeof value !== 'string') return undefined
  const key = value.trim().toLowerCase()
  return Object.hasOwn(ACTION_ALIASES, key) ? ACTION_ALIASES[key] : undefined
}

function appendToolRules(rules: PermissionRule[], tool: string, entry: unknown): void {
  const direct = normalizePermissionAction(entry)
  if (direct) {
    rules.push({ tool, pattern: WILDCARD, action: direct })
    return
  }
  if (!isPlainObject(entry)) return
  for (const [pattern, rawAction] of Object.entries(entry)) {
    const action = normalizePermissionAction(rawAction)
    if (action) rules.push({ tool, pattern, action })
  }
}

export function parsePermissions(raw: unknown): ParsedPermissions {
  const globalDirect = normalizePermissionAction(raw)
  if (globalDirect) {
    return { mode: 'global', globalAction: globalDirect, rules: [] }
  }

  if (!isPlainObject(raw)) {
    return { mode: 'rules', globalAction: 'allow', rules: [] }
  }

  const rules: PermissionRule[] = []
  for (const [tool, entry] of Object.entries(raw)) {
    appendToolRules(rules, tool, entry)
  }

  let globalAction: PermissionAction = 'allow'
  for (const rule of rules) {
    if (rule.tool === WILDCARD && rule.pattern === WILDCARD) globalAction = rule.action
  }

  return { mode: 'rules', globalAction, rules }
}

export function serializePermissions(parsed: ParsedPermissions): PermissionAction | JsonObject | undefined {
  if (parsed.mode === 'global') return parsed.globalAction
  if (parsed.rules.length === 0) return undefined

  const grouped = new Map<string, PermissionRule[]>()
  for (const rule of parsed.rules) {
    const tool = rule.tool.trim() || WILDCARD
    const pattern = rule.pattern.trim() || WILDCARD
    const bucket = grouped.get(tool)
    const normalized: PermissionRule = { tool, pattern, action: rule.action }
    if (bucket) {
      bucket.push(normalized)
      continue
    }
    grouped.set(tool, [normalized])
  }

  const out: JsonObject = {}
  for (const [tool, entries] of grouped) {
    const singleWildcard = entries.length === 1 && entries[0]?.pattern === WILDCARD
    if (singleWildcard) {
      setOwn(out, tool, entries[0]!.action)
      continue
    }
    const patternMap: JsonObject = {}
    for (const entry of entries) {
      setOwn(patternMap, entry.pattern, entry.action)
    }
    setOwn(out, tool, patternMap)
  }
  return out
}
