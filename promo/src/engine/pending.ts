// Captures a scene asked for that are not recorded yet. Films draw a labelled placeholder in
// their place, so a cut can be reviewed before the capture exists; the render reports the list.
const missing = new Set<string>()

export function notePending(name: string): void {
  missing.add(name)
}

export function pendingCaptures(): string[] {
  return [...missing].sort()
}
