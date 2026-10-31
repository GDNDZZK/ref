// The hero film: one main session starts helper sessions that work in parallel and report back.
// Pictures come from the `orchestra-*` capture scenarios and `crew` (see STORYBOARD.md); coordinates
// are fractions of the screenshot and are tuned against the real captures.
import { notificationCard } from '../components/notificationCard.ts'
import { sessionNameOf } from '../engine/screens.ts'
import { sideBySideShot } from './backend.ts'
import { FEATURES } from './features.ts'
import { filmScene, shotBeat, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.orchestra
const tag = tagOf(feature)

// Seconds into the crowd clip (the mock's fixed timeline): the fixer's card is up from 6 s, the
// capture moves focus to its tile at 7 and presses Allow at 8.5.
const CARD_UP = 6.2
const CARD_ANSWERED = 10.6
const CROWD_SECONDS = 12.5

export const scene = filmScene(feature, [
  // The task, typed into the main session.
  windowBeat({
    seconds: 4,
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
    seconds: 4.5,
    tag,
    screens: [{ name: 'gui-orchestra-plan' }],
    looks: [
      { at: 0, x: 0.6, y: 0.3, zoom: 1.5 },
      { at: 3.5, x: 0.6, y: 0.65, zoom: 1.5 },
    ],
    caption: { en: 'It plans, then starts helpers.', zh: '它先做计划，再启动帮手。' },
  }),
  // The crowd at work, recorded so that the tiles stream; one helper asks for approval.
  windowBeat({
    seconds: CROWD_SECONDS,
    tag,
    screens: [{ clip: 'gui-orchestra-crowd' }],
    looks: [
      { at: 0, x: 0.6, y: 0.48, zoom: 1.35 },
      { at: CARD_UP - 0.4, x: 0.6, y: 0.48, zoom: 1.35 },
      { at: CARD_UP + 0.4, x: 0.43, y: 0.52, zoom: 1.8 },
      { at: CARD_ANSWERED, x: 0.43, y: 0.52, zoom: 1.8 },
      { at: CARD_ANSWERED + 0.8, x: 0.6, y: 0.48, zoom: 1.35 },
    ],
    caption: [
      { from: 0.6, to: CARD_UP - 0.2, text: { en: 'They all work at once.', zh: '它们同时开工。' } },
      { from: CARD_UP + 0.4, to: CROWD_SECONDS - 0.4, text: { en: 'You only step in where it matters.', zh: '只有需要你的地方，才轮到你。' } },
    ],
  }),
  // The same crew in all three windows.
  shotBeat(4, (seconds) =>
    sideBySideShot(seconds, { en: 'Watch it from any window. Same crew.', zh: '在哪个窗口看都行，还是同一队。' }, 'crew', tag),
  ),
  // The reports arrive in the main session.
  windowBeat({
    seconds: 5.5,
    tag,
    screens: [{ name: 'gui-orchestra-reports' }, { name: 'gui-orchestra-inbox', at: 3.2 }],
    looks: [
      { at: 0, x: 0.6, y: 0.55, zoom: 1.4 },
      { at: 3.2, x: 0.6, y: 0.5, zoom: 1.2 },
    ],
    caption: { en: 'Reports come back to the main session.', zh: '汇报都回到主会话。' },
  }),
  // The wrap-up.
  windowBeat({
    seconds: 4.5,
    tag,
    screens: [{ name: 'gui-orchestra-done' }],
    looks: [{ at: 0, x: 0.6, y: 0.45, zoom: 1.4 }],
    extras: (shot) =>
      shot.add(notificationCard({ title: sessionNameOf('gui-orchestra-done'), body: { en: 'Finished', zh: '已完成' }, from: 1.8, to: 4.2 })),
    caption: { en: 'One answer. You stayed in one place.', zh: '一个结果，你没离开过原位。' },
  }),
])
