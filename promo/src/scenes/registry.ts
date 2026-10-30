// Scene ids and loaders. Kept free of browser imports because scripts/render.ts reads the ids.
import type { SceneDefinition } from '../engine/scene.ts'

export const sceneLoaders: Record<string, () => Promise<{ scene: SceneDefinition }>> = {
  demo: () => import('./demo.ts'),
}
