// The text of anything that was thrown.
export function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
