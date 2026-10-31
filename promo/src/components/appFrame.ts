import { lerp, span, track, type Key } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'
import type { Point } from './cursor.ts'
import { TITLE_BAR_HEIGHT_PX, createWindow } from './windowChrome.ts'

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

// Where the camera looks. (focusX, focusY) is a point of the picture, 0..1 each, that is
// centred in the window at `scale` (1 = the whole picture fits the window width).
export interface CameraKey {
  at: number
  scale: number
  focusX: number
  focusY: number
  ease?: Key['ease']
}

// A picture that fades in over the ones before it at time `from`; the first one is always shown.
export interface PictureLayer {
  src: string
  from: number
  // A recorded clip instead of a still: frame i shows from `from + i / fps` on.
  clip?: { frameUrl(index: number): string; frames: number; fps: number }
}

export interface AppFrameOptions {
  // All layers must have the size `imageSize`.
  layers: readonly PictureLayer[]
  // Pixel size of the picture; fixes the geometry before the file is loaded.
  imageSize: { width: number; height: number }
  title: string
  // Window position and size on the stage, including the title bar.
  rect: Rect
  camera: readonly CameraKey[]
  from: number
  to: number
}

const LAYER_FADE_SECONDS = 0.4
const ENTER_SECONDS = 0.8
const EXIT_SECONDS = 0.35
const ENTER_SCALE = 0.94

// Puts the clip frame for `elapsed` seconds into the clip on the picture and waits until it is decoded,
// so the screenshot that follows shows it.
async function showClipFrame(picture: HTMLImageElement, clip: NonNullable<PictureLayer['clip']>, elapsed: number) {
  const index = Math.min(clip.frames - 1, Math.max(0, Math.floor(elapsed * clip.fps)))
  if (picture.dataset.frame === String(index)) return
  picture.dataset.frame = String(index)
  picture.src = clip.frameUrl(index)
  await picture.decode()
}

export interface AppFrame extends Part {
  // A child that lives in the picture and moves with the camera (highlights, labels).
  attach(part: Part): void
  // Stage position of a picture point at `time`, for aiming the cursor.
  stagePoint(normalizedX: number, normalizedY: number, time: number): Point
}

// A window showing a screenshot that the camera pans and zooms over.
export function appFrame(options: AppFrameOptions): AppFrame {
  const { frame, body } = createWindow(options.title)
  const { rect } = options
  Object.assign(frame.style, {
    left: `${rect.x}px`,
    top: `${rect.y}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
  })
  const viewWidth = rect.width
  const viewHeight = rect.height - TITLE_BAR_HEIGHT_PX

  const world = element('div', 'app-world', body)
  const pictures = options.layers.map((layer) => {
    const picture = element('img', 'app-picture', world)
    picture.src = layer.src
    return picture
  })
  const attached: Part[] = []

  const scale = track(options.camera.map((key) => ({ at: key.at, value: key.scale, ease: key.ease })))
  const focusX = track(options.camera.map((key) => ({ at: key.at, value: key.focusX, ease: key.ease })))
  const focusY = track(options.camera.map((key) => ({ at: key.at, value: key.focusY, ease: key.ease })))

  // The picture is as wide as the window; its height follows its aspect ratio.
  const worldHeight = () => (viewWidth * options.imageSize.height) / options.imageSize.width

  // World offset (negative = scrolled) that centres the focus, without showing past the picture edges.
  const cameraOffset = (time: number): Point => {
    const zoom = scale(time)
    const scaledWidth = viewWidth * zoom
    const scaledHeight = worldHeight() * zoom
    const wanted = (focus: number, size: number, view: number) => view / 2 - focus * size
    const clamp = (offset: number, size: number, view: number) =>
      size <= view ? (view - size) / 2 : Math.min(0, Math.max(view - size, offset))
    return {
      x: clamp(wanted(focusX(time), scaledWidth, viewWidth), scaledWidth, viewWidth),
      y: clamp(wanted(focusY(time), scaledHeight, viewHeight), scaledHeight, viewHeight),
    }
  }

  return {
    el: frame,
    attach(part) {
      world.append(part.el)
      attached.push(part)
    },
    stagePoint(normalizedX, normalizedY, time) {
      const zoom = scale(time)
      const offset = cameraOffset(time)
      return {
        x: rect.x + offset.x + normalizedX * viewWidth * zoom,
        y: rect.y + TITLE_BAR_HEIGHT_PX + offset.y + normalizedY * worldHeight() * zoom,
      }
    },
    async render(time) {
      world.style.width = `${viewWidth}px`
      world.style.height = `${worldHeight()}px`
      const offset = cameraOffset(time)
      world.style.transform = `translate(${offset.x}px, ${offset.y}px) scale(${scale(time)})`
      for (const [index, layer] of options.layers.entries()) {
        const shown = index === 0 ? 1 : span(time, layer.from, LAYER_FADE_SECONDS, 'inOutSine')
        pictures[index]!.style.opacity = String(shown)
        if (layer.clip) await showClipFrame(pictures[index]!, layer.clip, time - layer.from)
      }

      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      frame.style.opacity = String(entering * (1 - leaving))
      frame.style.transform = `scale(${lerp(ENTER_SCALE, 1, entering)})`

      await Promise.all(attached.map((part) => part.render(time)))
    },
  }
}
