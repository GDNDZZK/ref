import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import sharp from 'sharp'

export const CLIPS_DIR = join(import.meta.dirname, '..', '..', 'assets', 'clips')
const MANIFEST_FILE = join(CLIPS_DIR, 'manifest.json')
export const CLIP_FRAMES_DIR = join(import.meta.dirname, '..', '..', '.cache', 'clip-frames')

// Frames per second of a stored clip; the capture photographs at the same rate.
export const CLIP_FPS = 15
const MS_PER_SECOND = 1000
const VP9_CRF = 24

// Turns the JPEG frames of a recording (each with the time it was taken, in ms) into a clip with
// an even frame rate: every output frame is the latest photograph taken by then. The clip is
// stored as WebM (VP9, 4:4:4 so text keeps its edges) in assets/clips and listed in the manifest.
export async function saveClip(name: string, framesDir: string, times: readonly number[]): Promise<void> {
  if (times.length === 0) throw new Error(`the recording "${name}" has no frames`)
  const stepMs = MS_PER_SECOND / CLIP_FPS
  const outputFrames = Math.ceil((times.at(-1)! + stepMs) / stepMs)
  const files = readdirSync(framesDir).filter((file) => file.endsWith('.jpg')).sort()
  if (files.length !== times.length) throw new Error(`${files.length} frames on disk, ${times.length} timestamps`)

  const lines: string[] = []
  let source = 0
  for (let output = 0; output < outputFrames; output++) {
    while (source + 1 < times.length && times[source + 1]! <= output * stepMs) source++
    lines.push(`file '${join(framesDir, files[source]!)}'`, `duration ${1 / CLIP_FPS}`)
  }
  lines.push(lines.at(-2)!) // the concat demuxer ignores the duration of the last entry
  const list = join(framesDir, 'list.txt')
  writeFileSync(list, `${lines.join('\n')}\n`)

  mkdirSync(CLIPS_DIR, { recursive: true })
  execFileSync(
    'ffmpeg',
    ['-v', 'error', '-y', '-f', 'concat', '-safe', '0', '-i', list, '-r', String(CLIP_FPS), '-vf', 'format=yuv444p',
      '-c:v', 'libvpx-vp9', '-crf', String(VP9_CRF), '-b:v', '0', '-deadline', 'good', '-cpu-used', '4', '-row-mt', '1',
      join(CLIPS_DIR, `${name}.webm`)],
    { stdio: 'inherit' },
  )

  const { width, height } = await sharp(join(framesDir, files[0]!)).metadata()
  const manifest: Record<string, unknown> = existsSync(MANIFEST_FILE) ? JSON.parse(readFileSync(MANIFEST_FILE, 'utf8')) : {}
  manifest[name] = { frames: outputFrames, fps: CLIP_FPS, width, height }
  const sorted = Object.fromEntries(Object.entries(manifest).sort(([a], [b]) => a.localeCompare(b)))
  writeFileSync(MANIFEST_FILE, `${JSON.stringify(sorted, null, 2)}\n`)
  rmSync(framesDir, { recursive: true, force: true })
}
