// The page's address mirrors what is open, so a reload (or a bookmark) comes back to it:
// ?session=<id> for the open session, ?view=split in split view. ?dir= stays as it is.

const SESSION_PARAM = 'session'
const VIEW_PARAM = 'view'
const SPLIT_VIEW = 'split'

export interface LocationState {
  sessionId?: string
  split?: boolean
}

function setOrDelete(params: URLSearchParams, name: string, value: string | undefined): void {
  if (value === undefined) params.delete(name)
  else params.set(name, value)
}

// Rewrites the search part of `url` for `state`; fields left out of `state` are kept.
export function withLocationState(url: URL, state: LocationState): URL {
  const result = new URL(url)
  if ('sessionId' in state) setOrDelete(result.searchParams, SESSION_PARAM, state.sessionId)
  if ('split' in state) setOrDelete(result.searchParams, VIEW_PARAM, state.split ? SPLIT_VIEW : undefined)
  return result
}

export function writeLocation(state: LocationState): void {
  const next = withLocationState(new URL(location.href), state)
  if (next.href !== location.href) history.replaceState(null, '', next)
}
