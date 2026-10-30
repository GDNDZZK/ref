import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import sharp from 'sharp'
import { REPO_ROOT } from './binaries.ts'

export const SCREENS_DIR = join(import.meta.dirname, '..', '..', 'assets', 'screens')
export const README_IMAGES_DIR = join(REPO_ROOT, 'docs', 'images')

// Width of the copies in docs/images. The READMEs show them at about 800 CSS px, so this
// is sharp on high-density screens and still small.
const README_IMAGE_WIDTH_PX = 1440

// Palette PNG: UI screenshots have few colours, 256 with dithering look the same and are ~4x smaller.
const PNG_OPTIONS = { palette: true, quality: 90, effort: 10, compressionLevel: 9 } as const

export interface PublishOptions {
  // Also write a smaller copy to docs/images/ for the READMEs.
  readme: boolean
}

// Saves a screenshot as assets/screens/<name>.png (full size, used by the videos) and,
// for README images, as docs/images/<name>.png.
export async function publishImage(png: Buffer, name: string, { readme }: PublishOptions): Promise<void> {
  mkdirSync(SCREENS_DIR, { recursive: true })
  await sharp(png).png(PNG_OPTIONS).toFile(join(SCREENS_DIR, `${name}.png`))
  if (!readme) return

  mkdirSync(README_IMAGES_DIR, { recursive: true })
  await sharp(png)
    .resize({ width: README_IMAGE_WIDTH_PX, withoutEnlargement: true })
    .png(PNG_OPTIONS)
    .toFile(join(README_IMAGES_DIR, `${name}.png`))
}
