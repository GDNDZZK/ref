import { notificationCard } from '../components/notificationCard.ts'
import { sessionNameOf } from '../engine/screens.ts'
import type { SceneDefinition, Step } from '../engine/scene.ts'
import { introShot } from '../shots/intro.ts'
import { outroShot } from '../shots/outro.ts'
import { sideBySideShot } from './backend.ts'
import { FEATURES } from './features.ts'
import { TRANSITION_SECONDS, cardsBeat, stepFor, tagOf, windowBeat } from './film.ts'

const F = FEATURES

// Seconds per feature beat; the trailer runs about a minute.
const FEATURE_SECONDS = 4.5
const INTRO_SECONDS = 3.5
const SIDE_BY_SIDE_SECONDS = 5.5

// The first twelve seconds: one task, a crew of helper sessions, the reports coming back.
const hook = [
  windowBeat({
    seconds: 2.5,
    screens: [{ name: 'gui-orchestra-task' }],
    looks: [
      { at: 0, x: 0.6, y: 0.8, zoom: 1.5 },
      { at: 3, x: 0.6, y: 0.82, zoom: 1.7 },
    ],
    caption: { en: 'One agent.', zh: '一个智能体，' },
  }),
  windowBeat({
    seconds: 6.5,
    screens: [{ clip: 'gui-orchestra-crowd' }],
    looks: [{ at: 0, x: 0.62, y: 0.47, zoom: 1.3 }],
    caption: [
      { from: 0.6, to: 3.1, text: { en: 'directs a whole crew.', zh: '指挥一整队帮手。' } },
      { from: 3.5, to: 6.6, text: { en: 'They work at once.', zh: '它们同时开工，' } },
    ],
  }),
  windowBeat({
    seconds: 3,
    screens: [{ name: 'gui-orchestra-reports' }],
    looks: [{ at: 0, x: 0.6, y: 0.55, zoom: 1.4 }],
    caption: { en: 'and report back.', zh: '再回来汇报。' },
  }),
]

// The closing climax: the whole crew, then the notification that everything is done.
const finale = windowBeat({
  seconds: 9.5,
  tag: tagOf(F.orchestra),
  screens: [{ name: 'gui-orchestra-six' }],
  looks: [
    { at: 0, x: 0.6, y: 0.5, zoom: 1.15 },
    { at: 9.5, x: 0.6, y: 0.5, zoom: 1.3 },
  ],
  extras: (shot) =>
    shot.add(notificationCard({ title: sessionNameOf('gui-orchestra-six'), body: { en: 'Finished', zh: '已完成' }, from: 3, to: 8.6 })),
  caption: { en: 'Close the window. Your crew keeps working.', zh: '关掉窗口，帮手们继续干活。' },
})

const beats = [
  windowBeat({
    seconds: FEATURE_SECONDS,
    tag: tagOf(F.permissions),
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1, x: 0.5, y: 0.6, zoom: 1.8 },
    ],
    marks: [{ x: 0.31, y: 0.7, width: 0.31, height: 0.055, from: 1.6, to: 3.9 }],
    caption: { en: 'You decide what runs.', zh: '能做什么，你说了算。' },
  }),
  windowBeat({
    seconds: FEATURE_SECONDS,
    tag: tagOf(F.edits),
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.45, y: 0.48, zoom: 2.2 },
    ],
    marks: [
      { x: 0.364, y: 0.487, width: 0.066, height: 0.028, from: 0.6, to: 3.6 },
      { x: 0.328, y: 0.577, width: 0.076, height: 0.028, from: 1, to: 3.6 },
    ],
    caption: { en: 'Edits address tagged lines.', zh: '按带标签的行号改文件。' },
  }),
  windowBeat({
    seconds: FEATURE_SECONDS,
    tag: tagOf(F.composer),
    screens: [{ name: 'gui-composer-slash' }, { name: 'gui-composer-at', at: 1.5 }, { name: 'gui-composer-shell', at: 3 }],
    looks: [{ at: 0, x: 0.6, y: 0.78, zoom: 1.6 }],
    caption: { en: '/ commands, @ files, ! shell.', zh: '/ 命令，@ 文件，! 命令行。' },
  }),
  windowBeat({
    seconds: FEATURE_SECONDS,
    tag: tagOf(F.longSessions),
    screens: [{ name: 'gui-context-full' }, { name: 'gui-context-compact', at: 2 }],
    looks: [
      { at: 0, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 1.6, x: 0.6, y: 0.8, zoom: 1.6 },
      { at: 2.6, x: 0.6, y: 0.3, zoom: 1.6 },
    ],
    caption: { en: 'Compaction and endless retries.', zh: '自动压缩，无限重试。' },
  }),
  cardsBeat({
    seconds: FEATURE_SECONDS,
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
    seconds: FEATURE_SECONDS,
    tag: tagOf(F.notify),
    screens: [{ name: 'gui-permission-card' }],
    looks: [{ at: 0, x: 0.6, y: 0.6, zoom: 1.4 }],
    extras: (shot) =>
      shot.add(
        notificationCard({
          title: sessionNameOf('gui-permission-card'),
          body: { en: 'Waiting for permission to use edit', zh: '等待授权使用 edit' },
          from: 0.9,
          to: 4,
        }),
      ),
    caption: { en: 'Called back when it needs you.', zh: '需要你时，通知你。' },
  }),
]

const FADE = { kind: 'fade', duration: TRANSITION_SECONDS } as const

export const scene: SceneDefinition = {
  id: 'trailer',
  steps: [
    // The film opens on the hook without a transition.
    ...hook.map((beat, index): Step => (index === 0 ? { shot: stepFor(beat).shot } : stepFor(beat))),
    { shot: introShot({ en: 'A coding agent with a backend of its own.', zh: '有独立后端的编程智能体。' }, { seconds: INTRO_SECONDS }), enter: FADE },
    {
      shot: sideBySideShot(SIDE_BY_SIDE_SECONDS, { en: 'One backend, every window.', zh: '一个后端，随处接入。' }),
      enter: FADE,
    },
    ...beats.map((beat): Step => stepFor(beat)),
    stepFor(finale),
    { shot: outroShot(), enter: { kind: 'rise', duration: TRANSITION_SECONDS } },
  ],
}
