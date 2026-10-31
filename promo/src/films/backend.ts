import castUrl from '../../assets/casts/tui-tools.cast?url'
import { appFrame } from '../components/appFrame.ts'
import { backdrop } from '../components/backdrop.ts'
import { caption } from '../components/caption.ts'
import { featureTag } from '../components/featureTag.ts'
import { serverPill } from '../components/serverPill.ts'
import { terminalView } from '../components/terminalView.ts'
import { TITLE_BAR_HEIGHT_PX } from '../components/windowChrome.ts'
import type { Localized } from '../engine/i18n.ts'
import { windowRect } from '../engine/layout.ts'
import type { SceneDefinition } from '../engine/scene.ts'
import { screen } from '../engine/screens.ts'
import { defineShot } from '../engine/shot.ts'
import { FEATURES } from './features.ts'
import { TRANSITION_SECONDS, tagOf } from './film.ts'
import { introShot } from '../shots/intro.ts'
import { outroShot } from '../shots/outro.ts'

const feature = FEATURES.backend
const tag = tagOf(feature)

const TERMINAL_FONT_PX = 21
const TERMINAL_POSITION = { x: 268, y: 90 }
// The cast opens with a splash that shows the random mock workspace path, and the prompt is sent
// at about 3 s. The replay starts after that and runs at half speed, so it lasts until the
// turn is done at about 6 s.
const CAST_START_SECONDS = 3.2
const CAST_SPEED = 0.5
const TERMINAL_SECONDS = 6
const SESSION_SECONDS_AT_CLOSE = 9

// Terminal turn, window closed, ref-server still counting, browser opens on the same session.
export function terminalToBrowserShot(seconds: number, captions: boolean) {
  const browserFrom = 8
  return defineShot(seconds, (shot) => {
    shot.add(backdrop())
    shot.add(
      terminalView({
        cast: castUrl,
        title: 'ref',
        ...TERMINAL_POSITION,
        fontSize: TERMINAL_FONT_PX,
        hideLastRows: 1,
        start: -CAST_START_SECONDS / CAST_SPEED,
        speed: CAST_SPEED,
        from: 0,
        to: TERMINAL_SECONDS,
      }),
    )
    shot.add(serverPill({ timerStart: SESSION_SECONDS_AT_CLOSE, from: 5, to: seconds }))
    const browser = screen('web-shared')
    shot.add(
      appFrame({
        layers: [{ src: browser.src, from: 0 }],
        imageSize: browser.size,
        title: 'ref-web',
        rect: windowRect(browser.size, TITLE_BAR_HEIGHT_PX),
        camera: [{ at: 0, scale: 1.15, focusX: 0.45, focusY: 0.3 }],
        from: browserFrom,
        to: seconds,
      }),
    )
    shot.add(featureTag({ ...tag, from: 0.1, to: seconds }))
    if (captions) {
      shot.add(
        caption([
          { from: 0.5, to: 5.4, text: { en: 'Start a task in the terminal.', zh: '在终端里开始一个任务。' } },
          { from: 5.8, to: 8.8, text: { en: 'Close the window. The session keeps working.', zh: '关掉窗口，会话继续运行。' } },
          { from: 9.2, to: seconds - 0.4, text: { en: 'Open it in a browser. Same session.', zh: '用浏览器打开，还是同一个会话。' } },
        ]),
      )
    }
  })
}

const WINDOW_WIDTH_PX = 600
const WINDOW_GAP_PX = 30
const WINDOW_TOP_PX = 280
const WINDOW_HEIGHT_PX = 470

interface SideBySide {
  name: string
  title: string
  zoom: number
  focus: { x: number; y: number }
}

// The terminal, the native app and the browser showing one session. Each gets a camera on the
// start of the transcript, since three whole windows would be too small to read.
const SIDE_BY_SIDE = {
  shared: [
    { name: 'tui-shared-clean', title: 'ref', zoom: 1.7, focus: { x: 0.3, y: 0.25 } },
    { name: 'gui-shared', title: 'ref-gui', zoom: 1.6, focus: { x: 0.6, y: 0.2 } },
    { name: 'web-shared', title: 'ref-web', zoom: 1.6, focus: { x: 0.6, y: 0.2 } },
  ],
  // The orchestrate session with its helpers: the GUI in split view, the browser with the helpers listed.
  crew: [
    { name: 'tui-crew-clean', title: 'ref', zoom: 1, focus: { x: 0.5, y: 0.5 } },
    { name: 'gui-crew', title: 'ref-gui', zoom: 1.3, focus: { x: 0.6, y: 0.4 } },
    { name: 'web-crew', title: 'ref-web', zoom: 1.02, focus: { x: 0.5, y: 0.3 } },
  ],
} as const satisfies Record<string, readonly SideBySide[]>
export type SideBySideSet = keyof typeof SIDE_BY_SIDE
const WINDOW_STAGGER_SECONDS = 0.5

export function sideBySideShot(seconds: number, text: string | Localized | undefined, set: SideBySideSet = 'shared', label: typeof tag = tag) {
  return defineShot(seconds, (shot) => {
    shot.add(backdrop())
    SIDE_BY_SIDE[set].forEach((entry, index) => {
      const picture = screen(entry.name)
      shot.add(
        appFrame({
          layers: [{ src: picture.src, from: 0 }],
          imageSize: picture.size,
          title: entry.title,
          rect: {
            x: WINDOW_GAP_PX + index * (WINDOW_WIDTH_PX + WINDOW_GAP_PX),
            y: WINDOW_TOP_PX,
            width: WINDOW_WIDTH_PX,
            height: WINDOW_HEIGHT_PX,
          },
          camera: [{ at: 0, scale: entry.zoom, focusX: entry.focus.x, focusY: entry.focus.y }],
          from: 0.2 + index * WINDOW_STAGGER_SECONDS,
          to: seconds,
        }),
      )
    })
    shot.add(featureTag({ ...label, from: 0.1, to: seconds }))
    if (text) shot.add(caption([{ from: 1.2, to: seconds - 0.4, text }]))
  })
}

const FIRST_SECONDS = 15
const SECOND_SECONDS = 6.5

export const scene: SceneDefinition = {
  id: feature.id,
  steps: [
    { shot: introShot(feature.tagline, { title: feature.name, logo: false, seconds: 3 }) },
    {
      shot: terminalToBrowserShot(FIRST_SECONDS + TRANSITION_SECONDS, true),
      enter: { kind: 'fade', duration: TRANSITION_SECONDS },
    },
    {
      shot: sideBySideShot(SECOND_SECONDS + TRANSITION_SECONDS, {
        en: 'Terminal, native app, browser. One backend.',
        zh: '终端、原生应用、浏览器，同一个后端。',
      }),
      enter: { kind: 'fade', duration: TRANSITION_SECONDS },
    },
    { shot: outroShot(), enter: { kind: 'rise', duration: TRANSITION_SECONDS } },
  ],
}
