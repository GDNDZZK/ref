import { FEATURES } from './features.ts'
import { filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.longSessions
const tag = tagOf(feature)

export const scene = filmScene(feature, [
  windowBeat({
    seconds: 8,
    tag,
    screens: [{ name: 'gui-context-full' }, { name: 'gui-context-compact', at: 4 }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 3.6, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 4.8, x: 0.6, y: 0.3, zoom: 1.6 },
    ],
    marks: [{ x: 0.77, y: 0.82, width: 0.14, height: 0.04, from: 1.6, to: 3.8 }],
    caption: [
      { from: 0.5, to: 3.8, text: { en: 'The context is almost full.', zh: '上下文快满了。' } },
      { from: 4.3, to: 7.6, text: { en: 'The next prompt compacts it first.', zh: '下一条提示会先压缩它。' } },
    ],
  }),
  windowBeat({
    seconds: 7,
    tag,
    screens: [{ name: 'gui-retry-status' }, { name: 'gui-retry-done', at: 4 }],
    looks: [
      { at: 0, x: 0.6, y: 0.6, zoom: 1.2 },
      { at: 1, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 3.6, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 4.6, x: 0.6, y: 0.15, zoom: 1.6 },
    ],
    marks: [{ x: 0.305, y: 0.81, width: 0.4, height: 0.04, from: 1.6, to: 3.8 }],
    caption: [
      { from: 0.5, to: 3.8, text: { en: 'Provider overloaded? It retries.', zh: '服务商过载？自动重试。' } },
      { from: 4.3, to: 6.6, text: { en: 'And carries on.', zh: '然后继续工作。' } },
    ],
  }),
  windowBeat({
    seconds: 5.5,
    tag,
    screens: [{ name: 'gui-long-output' }],
    looks: [
      { at: 0, x: 0.6, y: 0.25, zoom: 1.4 },
      { at: 5, x: 0.6, y: 0.75, zoom: 1.4 },
    ],
    caption: { en: 'Long output does not make the view jump.', zh: '输出再长，画面也不跳。' },
  }),
])
