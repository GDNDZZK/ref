// The hero film: one main session starts helper sessions that work in parallel and report back.
// Pictures come from the `orchestra` capture scenario (see STORYBOARD.md); coordinates are fractions
// of the screenshot and are tuned against the real captures.
import { filmScene, tagOf, windowBeat } from './film.ts'
import { notificationCard } from '../components/notificationCard.ts'
import { FEATURES } from './features.ts'

const feature = FEATURES.orchestra
const tag = tagOf(feature)

export const scene = filmScene(feature, [
  // The task, typed into the main session.
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-orchestra-task' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1, x: 0.6, y: 0.82, zoom: 1.6 },
    ],
    caption: { en: 'Give one session the task.', zh: '把任务交给一个会话。' },
  }),
  // Its plan and the create_session calls.
  windowBeat({
    seconds: 6,
    tag,
    screens: [{ name: 'gui-orchestra-plan' }],
    looks: [
      { at: 0, x: 0.6, y: 0.35, zoom: 1.5 },
      { at: 3.5, x: 0.6, y: 0.65, zoom: 1.5 },
    ],
    caption: { en: 'It plans, then starts helpers.', zh: '它先做计划，再启动帮手。' },
  }),
  // The crowd at work: a recording, so the tiles stream.
  windowBeat({
    seconds: 8,
    tag,
    screens: [{ clip: 'gui-orchestra-crowd' }],
    looks: [
      { at: 0, x: 0.55, y: 0.5, zoom: 1 },
      { at: 8, x: 0.58, y: 0.5, zoom: 1.12 },
    ],
    caption: { en: 'They all work at once.', zh: '它们同时开工。' },
  }),
  // One helper needs approval.
  windowBeat({
    seconds: 4,
    tag,
    screens: [{ name: 'gui-orchestra-permission' }, { name: 'gui-orchestra-allowed', at: 2.6 }],
    looks: [
      { at: 0, x: 0.55, y: 0.5, zoom: 1 },
      { at: 1, x: 0.5, y: 0.6, zoom: 1.7 },
    ],
    caption: { en: 'You only step in where it matters.', zh: '只有需要你的地方，才轮到你。' },
  }),
  // The reports arrive in the main session.
  windowBeat({
    seconds: 7,
    tag,
    screens: [{ name: 'gui-orchestra-reports' }, { name: 'gui-orchestra-inbox', at: 4.5 }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1.4 },
      { at: 4, x: 0.6, y: 0.5, zoom: 1.4 },
    ],
    caption: { en: 'Reports come back to the main session.', zh: '汇报都回到主会话。' },
  }),
  // The wrap-up.
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-orchestra-done' }],
    looks: [{ at: 0, x: 0.6, y: 0.45, zoom: 1.4 }],
    extras: (shot) =>
      shot.add(notificationCard({ title: 'main', body: { en: 'Finished', zh: '已完成' }, from: 2, to: 4.6 })),
    caption: { en: 'One answer. You stayed in one place.', zh: '一个结果，你没离开过原位。' },
  }),
])
