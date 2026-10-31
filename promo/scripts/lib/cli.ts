import { parseArgs } from 'node:util'
import { LANGS, isLang, type Lang } from '../../src/engine/i18n.ts'
import { sceneLoaders } from '../../src/scenes/registry.ts'

export interface RenderArgs {
  sceneIds: string[]
  langs: Lang[]
  fps: number
  workers: number
  outDir: string
  // When set, write one PNG of each of these seconds instead of a video.
  stillsAt?: number[]
}

const DEFAULT_FPS = 30
const DEFAULT_WORKERS = 4 // browser processes; 4 was fastest on a 12-core machine

export const USAGE = `Usage: pnpm render <scene...> [options]
       pnpm render:all [options]

Options:
  --lang en,zh   languages to render (default: all: ${LANGS.join(',')})
  --fps N        frame rate (default ${DEFAULT_FPS})
  --workers N    parallel browser processes (default ${DEFAULT_WORKERS})
  --out DIR      output directory (default out)
  --still SEC,.. write one PNG of each of those seconds instead of a video
Scenes: ${Object.keys(sceneLoaders).join(', ')}`

export function parseRenderArgs(argv: string[]): RenderArgs {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      all: { type: 'boolean' },
      lang: { type: 'string', default: LANGS.join(',') },
      fps: { type: 'string', default: String(DEFAULT_FPS) },
      workers: { type: 'string', default: String(DEFAULT_WORKERS) },
      out: { type: 'string', default: 'out' },
      still: { type: 'string' },
    },
  })

  const sceneIds = values.all ? Object.keys(sceneLoaders) : positionals
  if (sceneIds.length === 0) throw new Error(`no scene given\n${USAGE}`)
  const unknown = sceneIds.filter((id) => !(id in sceneLoaders))
  if (unknown.length > 0) throw new Error(`unknown scene: ${unknown.join(', ')}\n${USAGE}`)

  const langs = values.lang.split(',')
  const badLang = langs.find((lang) => !isLang(lang))
  if (badLang) throw new Error(`unknown language "${badLang}"`)

  return {
    sceneIds,
    langs: langs.filter(isLang),
    fps: Number(values.fps),
    workers: Number(values.workers),
    outDir: values.out,
    stillsAt: values.still === undefined ? undefined : parseSeconds(values.still),
  }
}

function parseSeconds(list: string): number[] {
  const seconds = list.split(',').map(Number)
  if (seconds.some((value) => !Number.isFinite(value))) throw new Error(`--still needs seconds, got "${list}"`)
  return seconds
}
