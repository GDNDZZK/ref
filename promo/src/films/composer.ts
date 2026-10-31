import { FEATURES } from './features.ts'
import { filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.composer
const tag = tagOf(feature)
// Every look keeps the text column (0.3..0.9 of the picture) in view.
const ZOOM = 1.6
const COLUMN_X = 0.6
const LOWER_Y = 0.78

export const scene = filmScene(feature, [
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-composer-slash' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1, x: COLUMN_X, y: LOWER_Y, zoom: ZOOM },
    ],
    caption: { en: '/ lists the commands.', zh: '/ 列出所有命令。' },
  }),
  windowBeat({
    seconds: 4,
    tag,
    screens: [{ name: 'gui-composer-at' }],
    looks: [{ at: 0, x: COLUMN_X, y: LOWER_Y, zoom: ZOOM }],
    caption: { en: '@ picks a file.', zh: '@ 引用文件。' },
  }),
  windowBeat({
    seconds: 6.5,
    tag,
    screens: [{ name: 'gui-composer-shell' }, { name: 'gui-composer-shell-output', at: 2.8 }],
    looks: [
      { at: 0, x: COLUMN_X, y: LOWER_Y, zoom: ZOOM },
      { at: 2.4, x: COLUMN_X, y: LOWER_Y, zoom: ZOOM },
      { at: 3.6, x: 0.5, y: 0.4, zoom: ZOOM },
    ],
    caption: [
      { from: 0.5, to: 2.6, text: { en: '! runs a command.', zh: '! 直接执行命令。' } },
      { from: 3.2, to: 6, text: { en: 'No model turn needed.', zh: '不需要模型参与。' } },
    ],
  }),
  windowBeat({
    seconds: 5.5,
    tag,
    title: 'ref',
    screens: [{ name: 'web-composer-image' }, { name: 'web-image-sent', at: 2.8 }],
    looks: [
      { at: 0, x: 0.58, y: 0.8, zoom: ZOOM },
      { at: 2.4, x: 0.58, y: 0.8, zoom: ZOOM },
      { at: 3.4, x: 0.58, y: 0.25, zoom: ZOOM },
    ],
    caption: { en: 'Paste an image.', zh: '直接粘贴图片。' },
  }),
  windowBeat({
    seconds: 4.5,
    tag,
    screens: [{ name: 'gui-composer-queue' }],
    looks: [
      { at: 0, x: 0.6, y: 0.7, zoom: 1 },
      { at: 1, x: COLUMN_X, y: LOWER_Y, zoom: ZOOM },
    ],
    marks: [{ x: 0.305, y: 0.815, width: 0.595, height: 0.045, from: 1.6, to: 4 }],
    caption: { en: 'Send while it works. The prompt waits.', zh: '它忙的时候也能发，消息会排队。' },
  }),
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-before-undo' }, { name: 'gui-composer-undo', at: 2.6 }],
    looks: [
      { at: 0, x: COLUMN_X, y: 0.25, zoom: ZOOM },
      { at: 2.2, x: COLUMN_X, y: 0.25, zoom: ZOOM },
      { at: 3.2, x: COLUMN_X, y: LOWER_Y, zoom: ZOOM },
    ],
    pointer: {
      path: [
        { at: 0, x: 0.6, y: 0.4 },
        { at: 0.6, x: 0.6, y: 0.4 },
        { at: 1.9, x: 0.83, y: 0.108, ease: 'outExpo' },
      ],
      clicks: [2.3],
    },
    caption: { en: 'Undo puts your draft back.', zh: '撤销会把草稿放回输入框。' },
  }),
])
