import { FEATURES } from './features.ts'
import { filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.sessions
const tag = tagOf(feature)

export const scene = filmScene(feature, [
  windowBeat({
    seconds: 9,
    tag,
    screens: [{ name: 'gui-split-view' }],
    looks: [
      { at: 0, x: 0.5, y: 0.5, zoom: 1 },
      { at: 1.2, x: 0.28, y: 0.28, zoom: 1.8 },
      { at: 4.6, x: 0.28, y: 0.28, zoom: 1.8 },
      { at: 6, x: 0.6, y: 0.5, zoom: 1 },
    ],
    caption: [
      { from: 0.5, to: 4.8, text: { en: 'Every session in the sidebar, live.', zh: '所有会话都在侧栏里，实时更新。' } },
      { from: 5.2, to: 8.6, text: { en: 'Or tile them and watch all at once.', zh: '也可以平铺，同时盯着全部。' } },
    ],
  }),
  windowBeat({
    seconds: 7,
    tag,
    screens: [{ name: 'gui-split-view' }, { name: 'gui-split-zoom', at: 3.4 }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1.2, x: 0.8, y: 0.76, zoom: 1.8 },
    ],
    marks: [{ x: 0.61, y: 0.55, width: 0.38, height: 0.43, from: 1.8, to: 6.3 }],
    caption: { en: 'The focused tile has its own prompt box.', zh: '选中的会话有自己的输入框。' },
  }),
  windowBeat({
    seconds: 7,
    tag,
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.28, y: 0.25, zoom: 1.8 },
      { at: 3, x: 0.28, y: 0.25, zoom: 1.8 },
      { at: 4.2, x: 0.5, y: 0.84, zoom: 2 },
    ],
    marks: [{ x: 0.005, y: 0.14, width: 0.19, height: 0.075, from: 1, to: 3.4 }],
    caption: { en: 'A session that needs you stands out.', zh: '需要你的会话会特别标出。' },
  }),
])
