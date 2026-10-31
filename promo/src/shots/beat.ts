import { appFrame, type AppFrame, type CameraKey, type Rect } from '../components/appFrame.ts'
import { backdrop } from '../components/backdrop.ts'
import { caption, type CaptionCue } from '../components/caption.ts'
import { cursor, pointerPath, type PathKey } from '../components/cursor.ts'
import { featureTag } from '../components/featureTag.ts'
import { highlight } from '../components/highlight.ts'
import { pendingFrame } from '../components/pendingFrame.ts'
import { TITLE_BAR_HEIGHT_PX } from '../components/windowChrome.ts'
import { windowClose } from '../components/windowClose.ts'
import type { EasingName } from '../engine/anim.ts'
import type { Localized } from '../engine/i18n.ts'
import { windowRect } from '../engine/layout.ts'
import { findClip } from '../engine/clips.ts'
import { findScreen } from '../engine/screens.ts'
import { defineShot, type ShotBuilder } from '../engine/shot.ts'

// Where the camera looks at time `at`: the picture point (x, y), 0..1 each, in the middle of
// the window, `zoom` times closer than "whole picture".
export interface Look {
  at: number
  x: number
  y: number
  zoom: number
  ease?: EasingName
}

// An outlined area of the picture, 0..1 each.
export interface Mark {
  x: number
  y: number
  width: number
  height: number
  label?: string | Localized
  from: number
  to: number
}

export interface Pointer {
  path: readonly (Omit<PathKey, 'x' | 'y'> & { x: number; y: number })[]
  clicks?: readonly number[]
}

export interface FeatureLabel {
  icon: string
  name: string | Localized
}

export interface BeatOptions {
  seconds: number
  // Pictures by name, or recorded clips (`clip`) that play from `at`; each fades in over the one before
  // it at `at`. All the same size.
  screens: readonly ({ name: string; at?: number } | { clip: string; at?: number })[]
  looks: readonly Look[]
  marks?: readonly Mark[]
  // Waypoints and click times in picture coordinates (0..1), so the pointer follows the camera.
  pointer?: Pointer
  // One line for the whole beat, or timed lines.
  caption?: string | Localized | readonly CaptionCue[]
  tag?: FeatureLabel
  title?: string
  // Window position on the stage; centred under the feature tag when left out.
  rect?: Rect
  // Size of the picture while the capture does not exist yet (a placeholder stands in).
  pendingSize?: { width: number; height: number }
  // When the window comes in (default 0; negative = already there).
  windowFrom?: number
  // N5: the window closes (shrinks to its centre) at this time.
  closeAt?: number
  // Leave out the backdrop when the beat is drawn into another shot.
  backdrop?: boolean
  // Anything else on the shot (a text card over the window, a notification).
  extras?: (shot: ShotBuilder, frame: AppFrame) => void
}

const CAPTION_IN_SECONDS = 0.5
const CAPTION_OUT_SECONDS = 0.4
const DEFAULT_WINDOW_TITLE = 'ref'
const DEFAULT_PENDING_SIZE = { width: 1280, height: 800 }

export function captionCues(given: NonNullable<BeatOptions['caption']>, seconds: number): readonly CaptionCue[] {
  if (typeof given === 'string' || !Array.isArray(given)) {
    return [{ from: CAPTION_IN_SECONDS, to: seconds - CAPTION_OUT_SECONDS, text: given as string | Localized }]
  }
  return given
}

// One beat of a film: a captured window, a camera move over it, optional marks, pointer,
// caption and feature tag.
export function beatShot(options: BeatOptions) {
  return defineShot(options.seconds, (shot) => {
    const entries = options.screens.map((entry) => {
      const at = entry.at ?? 0
      if ('clip' in entry) {
        const recorded = findClip(entry.clip)
        return { name: entry.clip, layer: recorded && { src: recorded.frameUrl(0), size: recorded.size, at, clip: recorded } }
      }
      const picture = findScreen(entry.name)
      return { name: entry.name, layer: picture && { ...picture, at, clip: undefined } }
    })
    if (entries.length === 0) throw new Error('a beat needs a picture')
    const layers = entries.flatMap((entry) => (entry.layer ? [entry.layer] : []))
    const missing = entries.filter((entry) => !entry.layer).map((entry) => entry.name)

    if (options.backdrop ?? true) shot.add(backdrop())
    const title = options.title ?? DEFAULT_WINDOW_TITLE
    const size = layers[0]?.size ?? options.pendingSize ?? DEFAULT_PENDING_SIZE
    const rect = options.rect ?? windowRect(size, TITLE_BAR_HEIGHT_PX)
    const camera: CameraKey[] = options.looks.map((look) => ({
      at: look.at,
      scale: look.zoom,
      focusX: look.x,
      focusY: look.y,
      ease: look.ease,
    }))
    const window: AppFrame =
      missing.length > 0
        ? pendingFrame({ names: missing, title, rect, from: options.windowFrom ?? 0, to: options.seconds })
        : appFrame({
            layers: layers.map((layer) => ({ src: layer.src, from: layer.at, clip: layer.clip })),
            imageSize: size,
            title,
            rect,
            camera,
            from: options.windowFrom ?? 0,
            to: options.seconds,
          })
    const frame = shot.add(options.closeAt === undefined ? window : windowClose(window, options.closeAt))
    for (const mark of options.marks ?? []) frame.attach(highlight(mark))

    const pointer = options.pointer
    if (pointer) {
      const path = pointerPath(pointer.path)
      shot.add(
        cursor((time) => {
          const point = path(time)
          return frame.stagePoint(point.x, point.y, time)
        }, pointer.clicks),
      )
    }
    if (options.tag) shot.add(featureTag({ ...options.tag, from: 0.1, to: options.seconds }))
    if (options.caption) shot.add(caption(captionCues(options.caption, options.seconds)))
    options.extras?.(shot, frame)
  })
}
