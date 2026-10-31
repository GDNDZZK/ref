import { entrance, placeSteps, type PlacedStep, type SceneDefinition } from './scene.ts'
import type { ShotInstance } from './shot.ts'

export const FRAME_WIDTH = 1920
export const FRAME_HEIGHT = 1080

export interface ScenePlayer {
  readonly duration: number
  // Shows the scene as it is `time` seconds in. Resolves when the frame can be screenshotted.
  seek(time: number): Promise<void>
}

interface MountedStep {
  placed: PlacedStep
  instance: ShotInstance
}

async function whenImagesDecoded(): Promise<void> {
  await Promise.all(Array.from(document.images, (image) => image.decode()))
}

export function mountScene(definition: SceneDefinition, stage: HTMLElement): ScenePlayer {
  const mounted: MountedStep[] = placeSteps(definition.steps).map((placed) => {
    const instance = placed.step.shot.mount()
    instance.root.style.display = 'none'
    stage.append(instance.root)
    return { placed, instance }
  })
  const duration = Math.max(...mounted.map(({ placed }) => placed.end))
  const overlay = definition.overlay?.mount()
  if (overlay) {
    overlay.root.classList.add('shot-overlay')
    stage.append(overlay.root)
  }

  return {
    duration,
    async seek(time) {
      const renders: Promise<void>[] = []
      for (const { placed, instance } of mounted) {
        const sinceStart = time - placed.start
        const visible = sinceStart >= 0 && time < placed.end
        instance.root.style.display = visible ? '' : 'none'
        if (!visible) continue

        const { opacity, offsetY } = entrance(placed.step.enter, sinceStart)
        instance.root.style.opacity = String(opacity)
        instance.root.style.transform = `translateY(${offsetY}px)`
        renders.push(instance.render(sinceStart))
      }
      if (overlay) renders.push(overlay.render(time))
      await Promise.all(renders)
      await document.fonts.ready
      await whenImagesDecoded()
    },
  }
}
