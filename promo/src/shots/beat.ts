import { appFrame, type AppFrame, type CameraKey } from '../components/appFrame.ts'
import { backdrop } from '../components/backdrop.ts'
import { caption, type CaptionCue } from '../components/caption.ts'
import { cursor, pointerPath, type PathKey } from '../components/cursor.ts'
import { featureTag } from '../components/featureTag.ts'
import { highlight } from '../components/highlight.ts'
import { TITLE_BAR_HEIGHT_PX } from '../components/windowChrome.ts'
import type { EasingName } from '../engine/anim.ts'
import type { Localized } from '../engine/i18n.ts'
import { windowRect } from '../engine/layout.ts'
import { screen } from '../engine/screens.ts'
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
  // Pictures by name; each fades in over the one before it at `at`. All the same size.
  screens: readonly { name: string; at?: number }[]
  looks: readonly Look[]
  marks?: readonly Mark[]
  // Waypoints and click times in picture coordinates (0..1), so the pointer follows the camera.
  pointer?: Pointer
  // One line for the whole beat, or timed lines.
  caption?: string | Localized | readonly CaptionCue[]
  tag?: FeatureLabel
  title?: string
  // Anything else on the shot (a text card over the window, a notification).
  extras?: (shot: ShotBuilder, frame: AppFrame) => void
}

const CAPTION_IN_SECONDS = 0.5
const CAPTION_OUT_SECONDS = 0.4
const DEFAULT_WINDOW_TITLE = 'ref'

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
    const layers = options.screens.map((entry) => ({ ...screen(entry.name), at: entry.at ?? 0 }))
    const first = layers[0]
    if (!first) throw new Error('a beat needs a picture')

    shot.add(backdrop())
    const camera: CameraKey[] = options.looks.map((look) => ({
      at: look.at,
      scale: look.zoom,
      focusX: look.x,
      focusY: look.y,
      ease: look.ease,
    }))
    const frame = shot.add(
      appFrame({
        layers: layers.map((layer) => ({ src: layer.src, from: layer.at })),
        imageSize: first.size,
        title: options.title ?? DEFAULT_WINDOW_TITLE,
        rect: windowRect(first.size, TITLE_BAR_HEIGHT_PX),
        camera,
        from: 0,
        to: options.seconds,
      }),
    )
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
