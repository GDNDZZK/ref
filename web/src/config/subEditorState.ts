// Tracks uncommitted drafts and JSON validation errors across visual ConfigDialog sub-editors
// so parent Save / Discard / dirty guards stay synchronized with nested inputs and textareas.

import {
  computed,
  inject,
  onBeforeUnmount,
  provide,
  reactive,
  ref,
  watch,
  type ComputedRef,
  type InjectionKey,
  type Ref,
} from 'vue'
import { parseJsoncObject } from './jsonc.ts'
import { formatJson, type JsonObject } from './jsonPatch.ts'

export interface SubEditorStatus {
  dirty: boolean
  committable: boolean
  invalid: boolean
}

interface ConfigSubEditorRegistry {
  readonly resetVersion: Readonly<Ref<number>>
  setStatus(scopeId: string, status: SubEditorStatus): void
  clearStatus(scopeId: string): void
  registerFlush(scopeId: string, flush: () => void): void
  unregisterFlush(scopeId: string): void
}

export interface ConfigSubEditorHost {
  readonly resetVersion: Readonly<Ref<number>>
  readonly hasUncommittedDraft: ComputedRef<boolean>
  readonly hasCommittableDraft: ComputedRef<boolean>
  readonly hasValidationError: ComputedRef<boolean>
  notifyExternalReset(): void
  flushPendingDrafts(): void
}

const CONFIG_SUB_EDITOR_KEY: InjectionKey<ConfigSubEditorRegistry> = Symbol('ConfigSubEditorRegistry')

let nextScopeSeq = 1

function canonicalJsonSnapshot(value: JsonObject | undefined): string {
  return value ? JSON.stringify(value) : ''
}

function clearRecord(record: Record<string, string>): void {
  for (const key of Object.keys(record)) {
    delete record[key]
  }
}

export function provideConfigSubEditorHost(): ConfigSubEditorHost {
  const resetVersion = ref(0)
  const statuses = reactive<Record<string, SubEditorStatus>>({})
  const flushCallbacks = new Map<string, () => void>()

  const registry: ConfigSubEditorRegistry = {
    resetVersion,
    setStatus(scopeId, status) {
      statuses[scopeId] = status
    },
    clearStatus(scopeId) {
      delete statuses[scopeId]
    },
    registerFlush(scopeId, flush) {
      flushCallbacks.set(scopeId, flush)
    },
    unregisterFlush(scopeId) {
      flushCallbacks.delete(scopeId)
    },
  }

  provide(CONFIG_SUB_EDITOR_KEY, registry)

  const hasUncommittedDraft = computed(() => Object.values(statuses).some((status) => status.dirty))
  const hasCommittableDraft = computed(() => Object.values(statuses).some((status) => status.committable))
  const hasValidationError = computed(() => Object.values(statuses).some((status) => status.invalid))

  function notifyExternalReset(): void {
    resetVersion.value += 1
  }

  function flushPendingDrafts(): void {
    for (const flush of flushCallbacks.values()) {
      flush()
    }
  }

  return {
    resetVersion,
    hasUncommittedDraft,
    hasCommittableDraft,
    hasValidationError,
    notifyExternalReset,
    flushPendingDrafts,
  }
}

export function useConfigSubEditorReset(): {
  hasHost: boolean
  resetVersion: Readonly<Ref<number>> | undefined
} {
  const registry = inject(CONFIG_SUB_EDITOR_KEY, undefined)
  return { hasHost: registry !== undefined, resetVersion: registry?.resetVersion }
}

export function useConfigSubEditorScope(
  statusGetter: () => SubEditorStatus,
  flush?: () => void,
): { hasHost: boolean; resetVersion: Readonly<Ref<number>> | undefined } {
  const registry = inject(CONFIG_SUB_EDITOR_KEY, undefined)
  if (!registry) return { hasHost: false, resetVersion: undefined }

  const scopeId = `sub-editor-${nextScopeSeq++}`
  watch(statusGetter, (next) => registry.setStatus(scopeId, next), { immediate: true })
  if (flush) registry.registerFlush(scopeId, flush)

  onBeforeUnmount(() => {
    registry.clearStatus(scopeId)
    registry.unregisterFlush(scopeId)
  })

  return { hasHost: true, resetVersion: registry.resetVersion }
}

export interface JsonObjectDraftOptions {
  allowEmpty: boolean
  invalidMessage: () => string
}

export function useJsonObjectDrafts(options: JsonObjectDraftOptions) {
  const drafts = reactive<Record<string, string>>({})
  const errors = reactive<Record<string, string>>({})
  const snapshots = new Map<string, string>()

  function read(key: string, currentObj: JsonObject | undefined, emptyFallback = ''): string {
    if (Object.hasOwn(drafts, key)) return drafts[key]!
    if (!currentObj) return emptyFallback
    return formatJson(currentObj).trimEnd()
  }

  function hasDraft(key: string): boolean {
    return Object.hasOwn(drafts, key)
  }

  function errorOf(key: string): string | undefined {
    return errors[key]
  }

  function update(
    key: string,
    text: string,
    currentObj: JsonObject | undefined,
    onValid: (parsed: JsonObject | undefined) => void,
  ): void {
    if (!snapshots.has(key)) snapshots.set(key, canonicalJsonSnapshot(currentObj))
    drafts[key] = text

    if (options.allowEmpty && text.trim() === '') {
      delete errors[key]
      snapshots.set(key, '')
      onValid(undefined)
      return
    }

    const parsed = parseJsoncObject(text)
    if (!parsed.ok) {
      errors[key] = options.invalidMessage()
      return
    }

    delete errors[key]
    const normalized = options.allowEmpty && Object.keys(parsed.value).length === 0 ? undefined : parsed.value
    snapshots.set(key, canonicalJsonSnapshot(normalized))
    onValid(normalized)
  }

  function blur(key: string): void {
    if (Object.hasOwn(errors, key)) return
    delete drafts[key]
    snapshots.delete(key)
  }

  function remove(key: string): void {
    delete drafts[key]
    delete errors[key]
    snapshots.delete(key)
  }

  function removeByPrefix(prefix: string): void {
    for (const key of Object.keys(drafts)) {
      if (key.startsWith(prefix)) remove(key)
    }
    for (const key of Object.keys(errors)) {
      if (key.startsWith(prefix)) remove(key)
    }
  }

  function renameKey(oldKey: string, newKey: string): void {
    if (oldKey === newKey) return
    if (Object.hasOwn(drafts, oldKey)) {
      drafts[newKey] = drafts[oldKey]!
      delete drafts[oldKey]
    }
    if (Object.hasOwn(errors, oldKey)) {
      errors[newKey] = errors[oldKey]!
      delete errors[oldKey]
    }
    if (snapshots.has(oldKey)) {
      snapshots.set(newKey, snapshots.get(oldKey)!)
      snapshots.delete(oldKey)
    }
  }

  function clearAll(): void {
    clearRecord(drafts)
    clearRecord(errors)
    snapshots.clear()
  }

  function reconcile(resolveObj: (key: string) => JsonObject | undefined, forceClear: boolean): void {
    if (forceClear) {
      clearAll()
      return
    }
    for (const key of Object.keys(drafts)) {
      if (canonicalJsonSnapshot(resolveObj(key)) !== snapshots.get(key)) {
        remove(key)
      }
    }
  }

  const hasInvalid = computed(() => Object.keys(errors).length > 0)

  return {
    read,
    hasDraft,
    errorOf,
    update,
    blur,
    remove,
    removeByPrefix,
    renameKey,
    clearAll,
    reconcile,
    hasInvalid,
  }
}

export interface InlineRenameOptions {
  normalize?: (raw: string) => string
  exists: (key: string) => boolean
  collisionMessage: () => string
  onCommit: (oldKey: string, newKey: string) => void
}

export function useInlineRenameDrafts(options: InlineRenameOptions) {
  const drafts = reactive<Record<string, string>>({})
  const normalize = options.normalize ?? ((raw: string) => raw.trim())

  function read(oldKey: string): string {
    return drafts[oldKey] ?? oldKey
  }

  function set(oldKey: string, value: string): void {
    drafts[oldKey] = value
  }

  function remove(oldKey: string): void {
    delete drafts[oldKey]
  }

  function clearAll(): void {
    clearRecord(drafts)
  }

  function targetOf(oldKey: string): string {
    return normalize(drafts[oldKey] ?? oldKey)
  }

  function hasCollision(oldKey: string): boolean {
    const target = targetOf(oldKey)
    return target !== '' && target !== oldKey && options.exists(target)
  }

  function errorOf(oldKey: string): string | undefined {
    return hasCollision(oldKey) ? options.collisionMessage() : undefined
  }

  function canCommit(oldKey: string): boolean {
    const target = targetOf(oldKey)
    return target !== '' && target !== oldKey && !options.exists(target)
  }

  function commit(oldKey: string): boolean {
    if (!canCommit(oldKey)) return false
    const target = targetOf(oldKey)
    delete drafts[oldKey]
    options.onCommit(oldKey, target)
    return true
  }

  function blur(oldKey: string): void {
    if (commit(oldKey)) return
    const target = targetOf(oldKey)
    if (target === '' || target === oldKey) delete drafts[oldKey]
  }

  function flushAll(): void {
    for (const oldKey of Object.keys(drafts)) {
      commit(oldKey)
    }
  }

  function reconcileKeys(validKeys: Iterable<string>, forceClear: boolean): void {
    if (forceClear) {
      clearAll()
      return
    }
    const valid = new Set(validKeys)
    for (const key of Object.keys(drafts)) {
      if (!valid.has(key)) delete drafts[key]
    }
  }

  const hasDirty = computed(() => Object.keys(drafts).some((oldKey) => targetOf(oldKey) !== oldKey))
  const hasCommittable = computed(() => Object.keys(drafts).some((oldKey) => canCommit(oldKey)))

  return {
    read,
    set,
    remove,
    clearAll,
    errorOf,
    canCommit,
    commit,
    blur,
    flushAll,
    reconcileKeys,
    hasDirty,
    hasCommittable,
  }
}
