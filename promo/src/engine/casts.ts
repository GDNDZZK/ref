// Terminal recordings (asciicast v2) written by capture/tui.ts into assets/casts.
const urls = import.meta.glob<string>('../../assets/casts/*.cast', { eager: true, query: '?url', import: 'default' })

// URL of the cast `name` (e.g. "tui-outage"), or undefined when it is not recorded yet.
// The terminal has no Chinese interface, so there is one cast per name.
export function findCast(name: string): string | undefined {
  return urls[`../../assets/casts/${name}.cast`]
}
