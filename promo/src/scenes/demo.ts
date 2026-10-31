// Pipeline check with placeholder material: every shared component once.
import placeholderUrl from '../../assets/placeholder/app.svg'
import { appFrame } from '../components/appFrame.ts'
import { backdrop } from '../components/backdrop.ts'
import { caption } from '../components/caption.ts'
import { cursor, pointerPath } from '../components/cursor.ts'
import { highlight } from '../components/highlight.ts'
import { typedText } from '../components/typedText.ts'
import type { SceneDefinition } from '../engine/scene.ts'
import { defineShot } from '../engine/shot.ts'
import { introShot } from '../shots/intro.ts'
import { outroShot } from '../shots/outro.ts'

const APP_SHOT_SECONDS = 6
const TRANSITION_SECONDS = 0.5
const APP_CAMERA_SETTLED_AT = 2.6
const COMPOSER = { x: 0.2125, y: 0.86, width: 0.7625, height: 0.084 }

const appShot = defineShot(APP_SHOT_SECONDS, (shot) => {
  shot.add(backdrop())
  const frame = shot.add(
    appFrame({
      layers: [{ src: placeholderUrl, from: 0 }],
      imageSize: { width: 1600, height: 1000 },
      title: 'ref',
      rect: { x: 240, y: 70, width: 1440, height: 944 },
      camera: [
        { at: 0, scale: 1, focusX: 0.5, focusY: 0.5 },
        { at: 1.2, scale: 1, focusX: 0.5, focusY: 0.5 },
        { at: 2.4, scale: 1.3, focusX: 0.55, focusY: 0.9 },
        { at: 5, scale: 1.3, focusX: 0.55, focusY: 0.9 },
      ],
      from: 0,
      to: APP_SHOT_SECONDS,
    }),
  )
  frame.attach(
    highlight({
      ...COMPOSER,
      label: { en: 'Prompt', zh: '输入框' },
      from: 2.6,
      to: 5.8,
    }),
  )
  const prompt = typedText({ text: 'Fix the failing test', start: 3.2, charsPerSecond: 12 })
  Object.assign(prompt.el.style, { left: '331px', top: '803px', fontSize: '20px' })
  frame.attach(prompt)

  const composerAim = frame.stagePoint(0.35, 0.9, APP_CAMERA_SETTLED_AT)
  shot.add(
    cursor(
      pointerPath([
        { at: 0, x: 1500, y: 900 },
        { at: 1.4, x: 1500, y: 900 },
        { at: 2.6, x: composerAim.x, y: composerAim.y, ease: 'outExpo' },
      ]),
      [2.8],
    ),
  )
  shot.add(
    caption([
      { from: 0.6, to: 2.6, text: { en: 'One backend, many windows', zh: '一个后端，多个窗口' } },
      { from: 3, to: 5.6, text: { en: 'Type a task and press enter', zh: '输入任务，回车即可' } },
    ]),
  )
})

export const scene: SceneDefinition = {
  id: 'demo',
  steps: [
    { shot: introShot({ en: 'A terminal coding agent', zh: '终端里的编程智能体' }) },
    { shot: appShot, enter: { kind: 'fade', duration: TRANSITION_SECONDS } },
    { shot: outroShot(), enter: { kind: 'rise', duration: TRANSITION_SECONDS } },
  ],
}
