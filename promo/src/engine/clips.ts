import manifest from '../../assets/clips/manifest.json'
import { getLang } from './i18n.ts'

// Absolute path of .cache/clips, where scripts/clips.ts unpacks the clips into JPEG frames
// (set in vite.config.ts).
declare const __CLIP_DIR__: string

interface ClipEntry {
  frames: number
  fps: number
  width: number
  height: number
}

const entries: Record<string, ClipEntry> = manifest

export interface Clip {
  name: string
  frames: number
  fps: number
  size: { width: number; height: number }
  // URL of frame `index` (0-based).
  frameUrl(index: number): string
}

function lookup(name: string): Clip | undefined {
  const entry = entries[name]
  if (!entry) return undefined
  return {
    name,
    frames: entry.frames,
    fps: entry.fps,
    size: { width: entry.width, height: entry.height },
    frameUrl: (index) => `/@fs${__CLIP_DIR__}/${name}/f${String(index + 1).padStart(4, '0')}.jpg`,
  }
}

// A recorded clip of the GUI by name, e.g. "gui-crowd"; Chinese renders take "-zh" when there is one.
export function clip(name: string): Clip {
  const found = (getLang() === 'zh' ? lookup(`${name}-zh`) : undefined) ?? lookup(name)
  if (!found) throw new Error(`no recorded clip "${name}"; run pnpm capture`)
  return found
}
