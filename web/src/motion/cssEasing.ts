// The generated `linear()` curves as strings, for the Web Animations API, which does not
// resolve `var()` in an easing.

const cache = new Map<string, string>()

// `name` as in theme.css: "--easing-out-expo", "--motion-ease".
export function cssEasing(name: string): string {
  const cached = cache.get(name)
  if (cached) return cached

  const probe = document.createElement('div')
  probe.style.setProperty('transition-timing-function', `var(${name})`)
  document.body.append(probe)
  const resolved = getComputedStyle(probe).transitionTimingFunction
  probe.remove()

  cache.set(name, resolved)
  return resolved
}
