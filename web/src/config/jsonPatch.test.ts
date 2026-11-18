import { describe, expect, it } from 'vitest'
import {
  BUILTIN_AGENT_META,
  BUILTIN_AGENTS,
  cloneJson,
  extractEnvRefs,
  formatJson,
  getAt,
  getBooleanAt,
  getNumberAt,
  getObjectAt,
  getStringArrayAt,
  getStringAt,
  getStringMapAt,
  isPlainObject,
  isSecretPlaceholder,
  normalizePermissionAction,
  parsePermissions,
  PROVIDER_PRESETS,
  serializePermissions,
  setAt,
  type JsonObject,
} from './jsonPatch.ts'

describe('isPlainObject & cloneJson', () => {
  it('identifies plain objects and rejects arrays, null, and class instances', () => {
    expect(isPlainObject({})).toBe(true)
    expect(isPlainObject(Object.create(null))).toBe(true)
    expect(isPlainObject([])).toBe(false)
    expect(isPlainObject(null)).toBe(false)
    expect(isPlainObject('text')).toBe(false)
    expect(isPlainObject(new Date())).toBe(false)
  })

  it('deep-clones nested structures without sharing references', () => {
    const original = {
      model: 'anthropic/claude-sonnet-4-5',
      instructions: ['AGENTS.md'],
      provider: { anthropic: { options: { apiKey: '{env:ANTHROPIC_API_KEY}' } } },
    }
    const copy = cloneJson(original)
    expect(copy).toEqual(original)
    copy.instructions.push('EXTRA.md')
    copy.provider.anthropic.options.apiKey = 'changed'
    expect(original.instructions).toEqual(['AGENTS.md'])
    expect(original.provider.anthropic.options.apiKey).toBe('{env:ANTHROPIC_API_KEY}')
  })
})

describe('getAt & typed getters', () => {
  const sample: JsonObject = {
    model: 'openai/gpt-5',
    compaction: { auto: true, recommend: 0.6 },
    instructions: ['a.md', 42, 'b.md'],
    headers: { Authorization: 'Bearer {env:TOKEN}', bad: 123 },
  }

  it('reads nested paths and returns undefined for missing or non-object paths', () => {
    expect(getAt(sample, [])).toBe(sample)
    expect(getStringAt(sample, ['model'])).toBe('openai/gpt-5')
    expect(getBooleanAt(sample, ['compaction', 'auto'])).toBe(true)
    expect(getNumberAt(sample, ['compaction', 'recommend'])).toBe(0.6)
    expect(getObjectAt(sample, ['compaction'])).toEqual({ auto: true, recommend: 0.6 })
    expect(getStringArrayAt(sample, ['instructions'])).toEqual(['a.md', 'b.md'])
    expect(getStringMapAt(sample, ['headers'])).toEqual({ Authorization: 'Bearer {env:TOKEN}' })
    expect(getAt(sample, ['model', 'sub'])).toBeUndefined()
    expect(getAt(sample, ['__proto__'])).toBeUndefined()
  })
})

describe('setAt & formatJson', () => {
  it('updates nested keys immutably while preserving untouched siblings, key order, and placeholders', () => {
    const root: JsonObject = {
      $name: 'Work profile',
      model: 'anthropic/claude-sonnet-4-5',
      provider: {
        anthropic: {
          options: {
            baseURL: 'https://api.anthropic.com/v1',
            apiKey: '{env:ANTHROPIC_API_KEY}',
            headers: { 'X-Team': '{file:~/.config/ref/team-id}' },
          },
          extraBody: { anthropic_beta: ['output-128k'] },
        },
      },
      customUnknownKey: { keepMe: true },
    }

    const updated = setAt(root, ['provider', 'anthropic', 'options', 'chunkTimeout'], 300000)

    expect(Object.keys(updated)).toEqual(['$name', 'model', 'provider', 'customUnknownKey'])
    expect(getAt(root, ['provider', 'anthropic', 'options', 'chunkTimeout'])).toBeUndefined()
    expect(getAt(updated, ['provider', 'anthropic', 'options', 'chunkTimeout'])).toBe(300000)
    expect(getAt(updated, ['provider', 'anthropic', 'options', 'apiKey'])).toBe('{env:ANTHROPIC_API_KEY}')
    expect(getAt(updated, ['provider', 'anthropic', 'options', 'headers'])).toEqual({
      'X-Team': '{file:~/.config/ref/team-id}',
    })
    expect(getAt(updated, ['provider', 'anthropic', 'extraBody'])).toEqual({
      anthropic_beta: ['output-128k'],
    })
    expect(getAt(updated, ['customUnknownKey'])).toEqual({ keepMe: true })
  })

  it('creates missing intermediate objects and prunes emptied intermediates on undefined', () => {
    const base: JsonObject = { model: 'openai/gpt-5' }
    const withRetry = setAt(base, ['retry', 'max_delay'], 600)
    expect(withRetry).toEqual({ model: 'openai/gpt-5', retry: { max_delay: 600 } })

    const cleared = setAt(withRetry, ['retry', 'max_delay'], undefined)
    expect(cleared).toEqual({ model: 'openai/gpt-5' })
    expect(Object.hasOwn(cleared, 'retry')).toBe(false)
  })

  it('never deletes untouched sibling keys or pre-existing empty siblings when pruning', () => {
    const root: JsonObject = {
      provider: {
        emptyProvider: {},
        custom: {
          name: 'Custom',
          options: { chunkTimeout: 10000 },
        },
      },
    }

    const afterTimeoutRemoved = setAt(root, ['provider', 'custom', 'options', 'chunkTimeout'], undefined)
    expect(afterTimeoutRemoved).toEqual({
      provider: {
        emptyProvider: {},
        custom: { name: 'Custom' },
      },
    })

    // Removing a missing key on an existing empty object must not prune that object.
    const untouchedEmpty = setAt(afterTimeoutRemoved, ['provider', 'emptyProvider', 'nonexistent'], undefined)
    expect(untouchedEmpty).toEqual(afterTimeoutRemoved)
  })

  it('formats 2-space indented JSON with trailing newline', () => {
    expect(formatJson({ model: 'openai/gpt-5' })).toBe('{\n  "model": "openai/gpt-5"\n}\n')
  })
})

describe('extractEnvRefs & isSecretPlaceholder', () => {
  it('recursively extracts sorted, deduplicated {env:VAR} names', () => {
    const doc = {
      provider: {
        work: {
          options: {
            apiKey: '{env:WORK_KEY}',
            headers: { Authorization: 'Bearer {env:AUTH_TOKEN} {env:WORK_KEY}' },
          },
        },
      },
      mcp: {
        local: {
          command: ['node', '{env:MCP_SCRIPT}'],
        },
      },
    }
    expect(extractEnvRefs(doc)).toEqual(['AUTH_TOKEN', 'MCP_SCRIPT', 'WORK_KEY'])
  })

  it('distinguishes {env:...} and {file:...} placeholders from literal API keys', () => {
    expect(isSecretPlaceholder('{env:ANTHROPIC_API_KEY}')).toBe(true)
    expect(isSecretPlaceholder('Bearer {env:DOCS_TOKEN}')).toBe(true)
    expect(isSecretPlaceholder('{file:~/.config/ref/team-id}')).toBe(true)
    expect(isSecretPlaceholder('{file:./my secret.txt}')).toBe(true)
    expect(isSecretPlaceholder('sk-ant-api03-literal-secret')).toBe(false)
    expect(isSecretPlaceholder('')).toBe(false)
    expect(isSecretPlaceholder('{env:}')).toBe(false)
    expect(isSecretPlaceholder('{file:   }')).toBe(false)
  })
})

describe('BUILTIN_AGENTS & PROVIDER_PRESETS', () => {
  it('lists all 5 built-in agents and 6 provider presets matching docs/configuration.md', () => {
    expect(BUILTIN_AGENTS).toEqual(['build', 'plan', 'general', 'explore', 'compaction'])
    expect(BUILTIN_AGENT_META.map((item) => item.name)).toEqual([...BUILTIN_AGENTS])

    expect(PROVIDER_PRESETS.map((preset) => preset.id)).toEqual([
      'anthropic',
      'openai',
      'deepseek',
      'openrouter',
      'ollama',
      'custom',
    ])
    const anthropic = PROVIDER_PRESETS.find((preset) => preset.id === 'anthropic')!
    expect(anthropic.suggestedModels).toEqual(['claude-sonnet-4-5', 'claude-opus-4-1', 'claude-haiku-4-5'])
    expect(anthropic.apiKeyPlaceholder).toBe('{env:ANTHROPIC_API_KEY}')
  })
})

describe('permissions codec', () => {
  it('normalizes permission action aliases from docs/configuration.md', () => {
    expect(normalizePermissionAction('allow')).toBe('allow')
    expect(normalizePermissionAction('AUTO')).toBe('allow')
    expect(normalizePermissionAction('yes')).toBe('allow')
    expect(normalizePermissionAction(true)).toBe('allow')
    expect(normalizePermissionAction('ask')).toBe('ask')
    expect(normalizePermissionAction('confirm')).toBe('ask')
    expect(normalizePermissionAction('prompt')).toBe('ask')
    expect(normalizePermissionAction('deny')).toBe('deny')
    expect(normalizePermissionAction('reject')).toBe('deny')
    expect(normalizePermissionAction('no')).toBe('deny')
    expect(normalizePermissionAction(false)).toBe('deny')
    expect(normalizePermissionAction('unknown')).toBeUndefined()
  })

  it('round-trips global string and boolean permissions', () => {
    const parsed = parsePermissions('confirm')
    expect(parsed).toEqual({ mode: 'global', globalAction: 'ask', rules: [] })
    expect(serializePermissions(parsed)).toBe('ask')

    expect(parsePermissions(false)).toEqual({ mode: 'global', globalAction: 'deny', rules: [] })
  })

  it('round-trips rule objects losslessly with compact wildcard strings and nested pattern maps', () => {
    const raw: JsonObject = {
      '*': 'allow',
      bash: { '*': 'ask', 'git status*': 'allow', 'rm *': 'deny' },
      'mcp_github_*': 'ask',
      webfetch: { 'https://docs.rs/*': 'allow' },
    }

    const parsed = parsePermissions(raw)
    expect(parsed).toEqual({
      mode: 'rules',
      globalAction: 'allow',
      rules: [
        { tool: '*', pattern: '*', action: 'allow' },
        { tool: 'bash', pattern: '*', action: 'ask' },
        { tool: 'bash', pattern: 'git status*', action: 'allow' },
        { tool: 'bash', pattern: 'rm *', action: 'deny' },
        { tool: 'mcp_github_*', pattern: '*', action: 'ask' },
        { tool: 'webfetch', pattern: 'https://docs.rs/*', action: 'allow' },
      ],
    })

    const serialized = serializePermissions(parsed)
    expect(serialized).toEqual(raw)
    expect(Object.keys(serialized as JsonObject)).toEqual(['*', 'bash', 'mcp_github_*', 'webfetch'])
    expect(Object.keys((serialized as JsonObject).bash as JsonObject)).toEqual(['*', 'git status*', 'rm *'])
  })

  it('returns undefined when serializing an empty rules list', () => {
    const empty = parsePermissions(undefined)
    expect(empty).toEqual({ mode: 'rules', globalAction: 'allow', rules: [] })
    expect(serializePermissions(empty)).toBeUndefined()
  })
})
