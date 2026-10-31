import castUrl from '../../assets/casts/tui-tools.cast?url'
import { backdrop } from '../components/backdrop.ts'
import { caption } from '../components/caption.ts'
import { notificationCard } from '../components/notificationCard.ts'
import { sessionNameOf } from '../engine/screens.ts'
import { serverPill } from '../components/serverPill.ts'
import { terminalView } from '../components/terminalView.ts'
import type { SceneDefinition, Step } from '../engine/scene.ts'
import { defineShot } from '../engine/shot.ts'
import { introShot } from '../shots/intro.ts'
import { outroShot } from '../shots/outro.ts'
import { sideBySideShot } from './backend.ts'
import { FEATURES } from './features.ts'
import { TRANSITION_SECONDS, cardsBeat, stepFor, tagOf, windowBeat } from './film.ts'

const F = FEATURES

const OPENER_SECONDS = 5
const OPENER_TERMINAL_SECONDS = 3.9
const OPENER_CAST_OFFSET_SECONDS = 4 // after the splash screen, which shows the mock path
const OPENER_FONT_PX = 21

// A turn runs in a terminal; the window closes; the server's timer keeps going.
const opener = defineShot(OPENER_SECONDS, (shot) => {
  shot.add(backdrop())
  shot.add(
    terminalView({
      cast: castUrl,
      title: 'ref',
      x: 268,
      y: 90,
      fontSize: OPENER_FONT_PX,
      hideLastRows: 1,
      start: -OPENER_CAST_OFFSET_SECONDS,
      from: 0,
      to: OPENER_TERMINAL_SECONDS,
    }),
  )
  shot.add(serverPill({ timerStart: 12, from: 2.8, to: OPENER_SECONDS }))
  shot.add(
    caption([
      { from: 0.6, to: OPENER_SECONDS - 0.4, text: { en: 'Close the window. It keeps working.', zh: '关掉窗口，它还在干活。' } },
    ]),
  )
})

const beats = [
  windowBeat({
    seconds: 5,
    tag: tagOf(F.sessions),
    screens: [{ name: 'gui-split-view' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1.2, x: 0.6, y: 0.5, zoom: 1.35 },
    ],
    caption: { en: 'Many sessions at once.', zh: '多个会话同时跑。' },
  }),
  windowBeat({
    seconds: 5.5,
    tag: tagOf(F.peers),
    screens: [{ name: 'gui-peer-message' }],
    looks: [
      { at: 0, x: 0.6, y: 0.3, zoom: 1.5 },
      { at: 4, x: 0.6, y: 0.55, zoom: 1.5 },
    ],
    caption: { en: 'Sessions message each other.', zh: '会话之间互发消息。' },
  }),
  windowBeat({
    seconds: 6,
    tag: tagOf(F.permissions),
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1, x: 0.5, y: 0.6, zoom: 1.8 },
    ],
    marks: [{ x: 0.31, y: 0.7, width: 0.31, height: 0.055, from: 1.8, to: 4.6 }],
    caption: { en: 'You decide what runs.', zh: '能做什么，你说了算。' },
  }),
  windowBeat({
    seconds: 5.5,
    tag: tagOf(F.edits),
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.45, y: 0.48, zoom: 2.2 },
    ],
    marks: [
      { x: 0.364, y: 0.487, width: 0.066, height: 0.028, from: 0.8, to: 4 },
      { x: 0.328, y: 0.577, width: 0.076, height: 0.028, from: 1.2, to: 4 },
    ],
    caption: { en: 'Edits address tagged lines.', zh: '按带标签的行号改文件。' },
  }),
  windowBeat({
    seconds: 6,
    tag: tagOf(F.composer),
    screens: [{ name: 'gui-composer-slash' }, { name: 'gui-composer-at', at: 2.2 }, { name: 'gui-composer-shell', at: 4 }],
    looks: [{ at: 0, x: 0.6, y: 0.78, zoom: 1.6 }],
    caption: { en: '/ commands, @ files, ! shell.', zh: '/ 命令，@ 文件，! 命令行。' },
  }),
  windowBeat({
    seconds: 5.5,
    tag: tagOf(F.longSessions),
    screens: [{ name: 'gui-context-full' }, { name: 'gui-context-compact', at: 2.6 }],
    looks: [
      { at: 0, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 2.2, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 3.4, x: 0.6, y: 0.3, zoom: 1.6 },
    ],
    caption: { en: 'Compaction and endless retries.', zh: '自动压缩，无限重试。' },
  }),
  cardsBeat({
    seconds: 5,
    tag: tagOf(F.setup),
    cards: [
      {
        x: 300,
        y: 330,
        width: 1320,
        title: 'opencode.json',
        lines: [
          { text: '{', typeAt: 0.4, charsPerSecond: 60 },
          { text: '  "model": "anthropic/claude-sonnet-4-5",', typeAt: 0.7, charsPerSecond: 60 },
          { text: '  "agent": { "build": { "variant": "high" } }', typeAt: 1.5, charsPerSecond: 60 },
          { text: '}', typeAt: 2.4, charsPerSecond: 60 },
        ],
      },
    ],
    caption: { en: 'Your opencode config works as is.', zh: '现有 opencode 配置直接可用。' },
  }),
  windowBeat({
    seconds: 5.5,
    tag: tagOf(F.notify),
    screens: [{ name: 'gui-permission-card' }],
    looks: [{ at: 0, x: 0.6, y: 0.6, zoom: 1.4 }],
    extras: (shot) =>
      shot.add(
        notificationCard({
          title: sessionNameOf('gui-permission-card'),
          body: { en: 'Waiting for permission to use edit', zh: '等待授权使用 edit' },
          from: 1,
          to: 5,
        }),
      ),
    caption: { en: 'Called back when it needs you.', zh: '需要你时，通知你。' },
  }),
]

const FADE = { kind: 'fade', duration: TRANSITION_SECONDS } as const

export const scene: SceneDefinition = {
  id: 'trailer',
  steps: [
    { shot: opener },
    { shot: introShot({ en: 'A coding agent with a backend of its own.', zh: '有独立后端的编程智能体。' }, { seconds: 4 }), enter: FADE },
    {
      shot: sideBySideShot(6.5, { en: 'One backend, every window.', zh: '一个后端，随处接入。' }),
      enter: FADE,
    },
    ...beats.map((beat): Step => stepFor(beat)),
    { shot: outroShot(), enter: { kind: 'rise', duration: TRANSITION_SECONDS } },
  ],
}
