// Scene ids and loaders. Kept free of browser imports because scripts/render.ts reads the ids.
import type { SceneDefinition } from '../engine/scene.ts'

const film = (name: string) => () => import(`../films/${name}.ts`) as Promise<{ scene: SceneDefinition }>

export const sceneLoaders: Record<string, () => Promise<{ scene: SceneDefinition }>> = {
  demo: () => import('./demo.ts'),
  intro: film('intro'),
  trailer: film('trailer'),
  crew: film('crew'),
  edits: film('edits'),
  outage: film('outage'),
  question: film('question'),
  permissions: film('permissions'),
  windows: film('windows'),
  opencode: film('opencode'),
  backend: film('backend'),
}
