// Captions that several films share (shooting script, sections 3 to 5).
import type { Localized } from '../engine/i18n.ts'

export const COPY = {
  hook: { en: 'Four agents, four tabs. One waited 14 minutes.', zh: '四个智能体，一个等你 14 分钟了。' },
  whatRefIs: { en: 'ref: a coding agent for terminal, desktop and browser.', zh: 'ref：终端、桌面、浏览器里的编程智能体。' },
  // The tagline on the brand title card (card text, not a caption).
  tagline: { en: 'A coding agent for terminal, desktop and browser.', zh: '终端、桌面、浏览器里的编程智能体。' },
  startsWorkers: { en: 'Its main agent starts worker sessions.', zh: '它的主智能体会自己开工作会话。' },
  watchAll: { en: 'Watch all four at once.', zh: '四个同时在跑，一屏看全。' },
  clickAndType: { en: 'Click a worker session and type to it.', zh: '点一个工作会话，直接给它发消息。' },
  ownTile: { en: 'Its permission request is in its own tile.', zh: '授权请求就出现在它自己的分块里。' },
  picksItUp: { en: 'The worker picks it up after its current step.', zh: '它做完这一步，就会读到你的消息。' },
  finished: { en: 'A notification says it finished.', zh: '做完了，会弹出通知。' },
  sameSessions: { en: 'Terminal, desktop app, browser: the same sessions.', zh: '终端、桌面端、浏览器，同一批会话。' },
} as const satisfies Record<string, Localized>

// Hero captions in film time, for films where the recording starts at film 3.4 (A, B, C1).
export const HERO_CAPTIONS = [
  [0.2, 3.3, COPY.hook],
  [3.5, 6.2, COPY.whatRefIs],
  [6.3, 8.8, COPY.startsWorkers],
  [8.9, 11.6, COPY.watchAll],
  [11.7, 14.9, COPY.clickAndType],
  [15.0, 18.2, COPY.ownTile],
  [18.3, 21.9, COPY.picksItUp],
] as const

// Film time of mock time 0 in A, B and C1.
export const HERO_FILM_START = 3.4
export const BRAND_TITLE_SECONDS = 2.6
