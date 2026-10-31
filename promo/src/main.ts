import '../../web/src/theme/generated/theme.css'
import '@xterm/xterm/css/xterm.css'
import './engine/styles.css'
import './components/components.css'
import { addDebugControls, fitStageToWindow } from './engine/debugControls.ts'
import { isLang, setLang } from './engine/i18n.ts'
import { pendingCaptures } from './engine/pending.ts'
import { mountScene } from './engine/player.ts'
import { sceneLoaders } from './scenes/registry.ts'

// Page contract for scripts/render.ts: when `window.promo` exists the scene is mounted.
declare global {
  interface Window {
    promo?: { duration: number; seek(time: number): Promise<void>; pending: string[] }
  }
}

const params = new URLSearchParams(location.search)
const sceneId = params.get('scene') ?? ''
const lang = params.get('lang') ?? 'en'

const loader = sceneLoaders[sceneId]
if (!loader) throw new Error(`unknown scene "${sceneId}"; known: ${Object.keys(sceneLoaders).join(', ')}`)
if (!isLang(lang)) throw new Error(`unknown lang "${lang}"`)

setLang(lang)
document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en'

const { scene } = await loader()
const stage = document.getElementById('stage')!
const player = mountScene(scene, stage)
await player.seek(0)
window.promo = { duration: player.duration, seek: player.seek, pending: pendingCaptures() }

if (params.has('debug')) {
  fitStageToWindow()
  addDebugControls(player)
}
