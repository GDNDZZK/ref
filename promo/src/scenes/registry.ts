// Scene ids and loaders. Kept free of browser imports because scripts/render.ts reads the ids.
import type { SceneDefinition } from '../engine/scene.ts'

const film = (name: string) => () => import(`../films/${name}.ts`) as Promise<{ scene: SceneDefinition }>

export const sceneLoaders: Record<string, () => Promise<{ scene: SceneDefinition }>> = {
  demo: () => import('./demo.ts'),
  trailer: film('trailer'),
  backend: film('backend'),
  sessions: film('sessions'),
  peers: film('peers'),
  permissions: film('permissions'),
  edits: film('edits'),
  composer: film('composer'),
  'long-sessions': film('long-sessions'),
  setup: film('setup'),
  notify: film('notify'),
}
