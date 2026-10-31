// Terminal, desktop app and browser side by side on one backend.
import { backdrop } from '../components/backdrop.ts'
import { TITLE_BAR_HEIGHT_PX } from '../components/windowChrome.ts'
import { defineShot } from '../engine/shot.ts'
import { beatShot, type Look } from './beat.ts'

export interface WindowPicture {
  // Capture names, by interface. Later pictures fade in over earlier ones at `at`.
  screens: readonly ({ name: string; at?: number } | { clip: string; at?: number })[]
  title: string
  look: Look
  // N5: the window closes at this time.
  closeAt?: number
}

export const THREE_WINDOWS_LAYOUT = { width: 600, gap: 30, top: 250, height: 520 }
const STAGGER_SECONDS = 0.35

// Window rectangles left to right.
export function threeWindowRects() {
  const { width, gap, top, height } = THREE_WINDOWS_LAYOUT
  return [0, 1, 2].map((index) => ({ x: gap + index * (width + gap), y: top, width, height }))
}

// One shot with the three windows; each window is a beat of its own drawn into the shot, so
// pictures that are not captured yet get a placeholder.
export function threeWindowsShot(seconds: number, windows: readonly WindowPicture[], entrance = true) {
  const rects = threeWindowRects()
  return defineShot(seconds, (shot) => {
    shot.add(backdrop())
    const inner = windows.map((window, index) =>
      beatShot({
        seconds,
        screens: window.screens,
        looks: [window.look],
        title: window.title,
        rect: rects[index],
        pendingSize: { width: rects[index]!.width, height: rects[index]!.height - TITLE_BAR_HEIGHT_PX },
        closeAt: window.closeAt,
        windowFrom: entrance ? index * STAGGER_SECONDS : -1,
        backdrop: false,
      }).mount(),
    )
    inner.forEach((instance) => {
      instance.root.classList.add('shot-overlay')
      shot.add({ el: instance.root, render: (time) => instance.render(time) })
    })
  })
}
