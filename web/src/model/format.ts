// Numbers and times as the conversation shows them (same rules as the TUI's text.rs).

const MS_PER_SECOND = 1000
const SECONDS_PER_MINUTE = 60
const SECONDS_PER_HOUR = 3600
const TENTH_SECOND_LIMIT_MS = 10_000
const KILO = 1000
const MEGA_ROUNDING_LIMIT = 999_950
const MEGA = 1_000_000

const pad2 = (n: number) => String(n).padStart(2, '0')

// `850ms`, `1.4s`, `12s`, `2m 03s`, `1h 02m`.
export function formatDuration(ms: number): string {
  const clamped = Math.max(0, ms)
  if (clamped < MS_PER_SECOND) return `${Math.floor(clamped)}ms`
  if (clamped < TENTH_SECOND_LIMIT_MS) return `${(clamped / MS_PER_SECOND).toFixed(1)}s`
  return formatElapsed(clamped)
}

// Coarse, for live counters: `3s`, `1m 05s`, `1h 02m`.
export function formatElapsed(ms: number): string {
  const seconds = Math.floor(Math.max(0, ms) / MS_PER_SECOND)
  if (seconds < SECONDS_PER_MINUTE) return `${seconds}s`
  if (seconds < SECONDS_PER_HOUR)
    return `${Math.floor(seconds / SECONDS_PER_MINUTE)}m ${pad2(seconds % SECONDS_PER_MINUTE)}s`
  const minutes = Math.floor(seconds / SECONDS_PER_MINUTE)
  return `${Math.floor(seconds / SECONDS_PER_HOUR)}h ${pad2(minutes % SECONDS_PER_MINUTE)}m`
}

// `950`, `12.3k`, `1.0M`.
export function formatTokens(count: number): string {
  if (count >= MEGA_ROUNDING_LIMIT) return `${(count / MEGA).toFixed(1)}M`
  if (count >= KILO) return `${(count / KILO).toFixed(1)}k`
  return String(count)
}

// `HH:mm:ss`, local time.
export function formatClock(ms: number): string {
  const time = new Date(ms)
  return `${pad2(time.getHours())}:${pad2(time.getMinutes())}:${pad2(time.getSeconds())}`
}

// Whole seconds left until `at`, rounded up.
export function secondsUntil(at: number, now: number): number {
  return Math.max(0, Math.ceil((at - now) / MS_PER_SECOND))
}

const BYTES_PER_KIB = 1024

// `512 B`, `12.3 KB`, `1.5 MB`.
export function formatBytes(bytes: number): string {
  if (bytes < BYTES_PER_KIB) return `${bytes} B`
  if (bytes < BYTES_PER_KIB ** 2) return `${(bytes / BYTES_PER_KIB).toFixed(1)} KB`
  return `${(bytes / BYTES_PER_KIB ** 2).toFixed(1)} MB`
}
