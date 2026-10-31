import { FEATURES } from './features.ts'
import { filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.peers
const tag = tagOf(feature)

export const scene = filmScene(feature, [
  windowBeat({
    seconds: 14,
    tag,
    screens: [{ name: 'gui-peer-message' }],
    looks: [
      { at: 0, x: 0.6, y: 0.4, zoom: 1 },
      { at: 1.2, x: 0.6, y: 0.25, zoom: 1.6 },
      { at: 5, x: 0.6, y: 0.25, zoom: 1.6 },
      { at: 6.4, x: 0.6, y: 0.5, zoom: 1.7 },
      { at: 11, x: 0.6, y: 0.5, zoom: 1.7 },
    ],
    marks: [
      { x: 0.305, y: 0.18, width: 0.595, height: 0.05, from: 2.2, to: 5.6 },
      { x: 0.305, y: 0.4, width: 0.595, height: 0.175, from: 7.2, to: 12.6 },
    ],
    caption: [
      { from: 0.5, to: 5.8, text: { en: 'The agent can start a helper session.', zh: '智能体可以启动帮手会话。' } },
      { from: 6.2, to: 13.2, text: { en: 'Its report arrives as a message from that session.', zh: '帮手的结果，作为来自它的消息送达。' } },
    ],
  }),
  windowBeat({
    seconds: 6,
    tag,
    screens: [{ name: 'gui-peers-inbox' }],
    looks: [
      { at: 0, x: 0.5, y: 0.45, zoom: 1 },
      { at: 1, x: 0.5, y: 0.35, zoom: 2 },
    ],
    caption: { en: 'Inbox and outbox show every message.', zh: '收件箱和发件箱里能查到每条消息。' },
  }),
])
