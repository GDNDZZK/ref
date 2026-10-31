// Unpacks the clips in assets/clips (WebM) into numbered JPEG frames under .cache/clips, which the
// scene pages load frame by frame. Skips a clip whose frames are newer than its file.
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const PROMO_DIR = join(import.meta.dirname, '..')
const SOURCE_DIR = join(PROMO_DIR, 'assets', 'clips')
const FRAMES_DIR = join(PROMO_DIR, '.cache', 'clips')
const STAMP = '.done'
const JPEG_QUALITY = '2' // ffmpeg -q:v, 2 is near lossless

export function prepareClips(): void {
  if (!existsSync(SOURCE_DIR)) return
  for (const file of readdirSync(SOURCE_DIR).filter((entry) => entry.endsWith('.webm'))) {
    const name = file.slice(0, -'.webm'.length)
    const target = join(FRAMES_DIR, name)
    const source = join(SOURCE_DIR, file)
    const stamp = join(target, STAMP)
    if (existsSync(stamp) && statSync(stamp).mtimeMs >= statSync(source).mtimeMs) continue

    rmSync(target, { recursive: true, force: true })
    mkdirSync(target, { recursive: true })
    execFileSync('ffmpeg', ['-v', 'error', '-i', source, '-q:v', JPEG_QUALITY, join(target, 'f%04d.jpg')], { stdio: 'inherit' })
    writeFileSync(stamp, '')
    utimesSync(stamp, new Date(), new Date())
    console.log(`unpacked clip ${name}`)
  }
}

if (import.meta.main) prepareClips()
