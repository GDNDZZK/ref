import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import sharp from 'sharp'
import { REPO_ROOT } from './binaries.ts'

export const SCREENS_DIR = join(import.meta.dirname, '..', '..', 'assets', 'screens')
// name -> pixel size of every picture in assets/screens, read by the scenes.
export const MANIFEST_FILE = join(SCREENS_DIR, 'manifest.json')
export const README_IMAGES_DIR = join(REPO_ROOT, 'docs', 'images')

// Width of the copies in docs/images. The READMEs show them at about 800 CSS px, so this
// is sharp on high-density screens and still small.
const README_IMAGE_WIDTH_PX = 1440

// Palette PNG: UI screenshots have few colours, 256 with dithering look the same and are ~4x smaller.
const PNG_OPTIONS = { palette: true, quality: 90, effort: 10, compressionLevel: 9 } as const

export interface PublishOptions {
  // Also write a smaller copy to docs/images/ for the READMEs.
  readme: boolean
  // Name of the session on screen (random in a mock run). The films that draw a notification or a
  // browser tab for this picture need the same name.
  sessionName?: string
}

// Saves a screenshot as assets/screens/<name>.png (full size, used by the videos) and,
// for README images, as docs/images/<name>.png.
export async function publishImage(png: Buffer, name: string, { readme, sessionName }: PublishOptions): Promise<void> {
  mkdirSync(SCREENS_DIR, { recursive: true })
  await sharp(png).png(PNG_OPTIONS).toFile(join(SCREENS_DIR, `${name}.png`))
  const { width, height } = await sharp(png).metadata()
  recordSize(name, { width, height, sessionName })
  if (!readme) return

  mkdirSync(README_IMAGES_DIR, { recursive: true })
  await sharp(png)
    .resize({ width: README_IMAGE_WIDTH_PX, withoutEnlargement: true })
    .png(PNG_OPTIONS)
    .toFile(join(README_IMAGES_DIR, `${name}.png`))
}

function recordSize(name: string, size: { width: number; height: number; sessionName?: string }): void {
  const manifest: Record<string, unknown> = existsSync(MANIFEST_FILE) ? JSON.parse(readFileSync(MANIFEST_FILE, 'utf8')) : {}
  manifest[name] = size
  const sorted = Object.fromEntries(Object.entries(manifest).sort(([a], [b]) => a.localeCompare(b)))
  writeFileSync(MANIFEST_FILE, `${JSON.stringify(sorted, null, 2)}\n`)
}

// The terminal's last row shows the mock workspace path (random per run). The videos and the
// README images use this copy without that row.
const TERMINAL_FOOTER_PX = 80 // last row (2 x 19 px) plus the 18 px padding, at 2x

export async function publishWithoutFooter(name: string, { readme }: PublishOptions): Promise<void> {
  const source = join(SCREENS_DIR, `${name}.png`)
  const { width, height } = await sharp(source).metadata()
  const cropped = { width, height: height - TERMINAL_FOOTER_PX }
  const clean = await sharp(source).extract({ left: 0, top: 0, ...cropped }).png(PNG_OPTIONS).toBuffer()
  await sharp(clean).toFile(join(SCREENS_DIR, `${name}-clean.png`))
  recordSize(`${name}-clean`, cropped)
  if (!readme) return

  mkdirSync(README_IMAGES_DIR, { recursive: true })
  await sharp(clean)
    .resize({ width: README_IMAGE_WIDTH_PX, withoutEnlargement: true })
    .png(PNG_OPTIONS)
    .toFile(join(README_IMAGES_DIR, `${name}.png`))
}
