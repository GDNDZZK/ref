import manifest from '../../assets/screens/manifest.json'
import { getLang } from './i18n.ts'

const urls = import.meta.glob<string>('../../assets/screens/*.png', { eager: true, query: '?url', import: 'default' })
const sizes: Record<string, { width: number; height: number; sessionName?: string }> = manifest

export interface Screen {
  src: string
  size: { width: number; height: number }
  // The session on screen, where the capture knew it (random per mock run).
  sessionName?: string
}

function lookup(fileName: string): Screen | undefined {
  const src = urls[`../../assets/screens/${fileName}.png`]
  const size = sizes[fileName]
  return src && size ? { src, size: { width: size.width, height: size.height }, sessionName: size.sessionName } : undefined
}

// A captured picture by name, e.g. "gui-permission-card". Chinese renders take the "-zh"
// capture when there is one (the TUI has no Chinese interface).
export function screen(name: string): Screen {
  const found = findScreen(name)
  if (!found) throw new Error(`no captured picture "${name}"; run pnpm capture`)
  return found
}

// Like `screen`, but undefined when the picture is not captured yet.
export function findScreen(name: string): Screen | undefined {
  return (getLang() === 'zh' ? lookup(`${name}-zh`) : undefined) ?? lookup(name)
}

// The session shown in a captured picture. The name is random per capture, so a film that draws
// something about that session (a notification, the page title) takes it from here.
export function sessionNameOf(name: string): string {
  const sessionName = screen(name).sessionName
  if (!sessionName) throw new Error(`no session name recorded for "${name}"; run pnpm capture`)
  return sessionName
}
