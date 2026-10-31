import { browserTab } from '../components/browserTab.ts'
import { notificationCard } from '../components/notificationCard.ts'
import { FEATURES } from './features.ts'
import { filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.notify
const tag = tagOf(feature)
// Texts from gui/src/notify/notification_text.cpp and web/src/i18n/locales.
// The session in the picture; a notification is titled with its session.
const SESSION_TITLE = 'workspace-161a'
const NOTIFICATION_BODY = { en: 'Waiting for permission to use edit', zh: '等待授权使用 edit' }

export const scene = filmScene(feature, [
  windowBeat({
    seconds: 8,
    tag,
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1.2, x: 0.6, y: 0.6, zoom: 1.4 },
    ],
    extras: (shot) =>
      shot.add(notificationCard({ title: SESSION_TITLE, body: NOTIFICATION_BODY, from: 3, to: 7.6 })),
    caption: [
      { from: 0.5, to: 2.8, text: { en: 'Go and do something else.', zh: '去忙别的。' } },
      { from: 3.2, to: 7.6, text: { en: 'A notification calls you back.', zh: '需要你时，通知会叫你回来。' } },
    ],
  }),
  windowBeat({
    seconds: 6,
    tag,
    screens: [{ name: 'web-permission-card' }],
    looks: [{ at: 0, x: 0.55, y: 0.5, zoom: 1.3 }],
    extras: (shot) => shot.add(browserTab({ title: 'ref', alertAt: 2.2, from: 0.3, to: 6.5 })),
    caption: [
      { from: 0.5, to: 2, text: { en: 'In a browser tab, ...', zh: '在浏览器标签页里，' } },
      { from: 2.4, to: 5.6, text: { en: 'the title and icon ask for attention.', zh: '标题和图标会提醒你。' } },
    ],
  }),
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-settings' }],
    looks: [
      { at: 0, x: 0.5, y: 0.42, zoom: 1 },
      { at: 1, x: 0.5, y: 0.42, zoom: 1.8 },
    ],
    marks: [{ x: 0.32, y: 0.42, width: 0.37, height: 0.24, from: 1.8, to: 4.6 }],
    caption: { en: 'Choose what you hear about.', zh: '想听到哪些，自己选。' },
  }),
])
