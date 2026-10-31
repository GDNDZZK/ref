import manifest from '../../assets/screens/manifest.json'
import { getLang } from './i18n.ts'

const urls = import.meta.glob<string>('../../assets/screens/*.png', { eager: true, query: '?url', import: 'default' })
const sizes: Record<string, { width: number; height: number }> = manifest

export interface Screen {
  src: string
  size: { width: number; height: number }
}

function lookup(fileName: string): Screen | undefined {
  const src = urls[`../../assets/screens/${fileName}.png`]
  const size = sizes[fileName]
  return src && size ? { src, size } : undefined
}

// A captured picture by name, e.g. "gui-permission-card". Chinese renders take the "-zh"
// capture when there is one (the TUI has no Chinese interface).
export function screen(name: string): Screen {
  const found = (getLang() === 'zh' ? lookup(`${name}-zh`) : undefined) ?? lookup(name)
  if (!found) throw new Error(`no captured picture "${name}"; run pnpm capture`)
  return found
}
