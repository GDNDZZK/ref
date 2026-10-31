import { FEATURES } from './features.ts'
import { cardsBeat, filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.permissions
const tag = tagOf(feature)

// docs/configuration.md, "Permissions".
const POLICY_LINES = [
  '"permission": {',
  '  "edit": "ask",',
  '  "bash": { "*": "ask", "git diff*": "allow", "rm *": "deny" },',
  '  "webfetch": { "*": "ask", "https://docs.rs/*": "allow" }',
  '}',
]
const POLICY_TYPE_START = 0.8
const POLICY_CHARS_PER_SECOND = 55

export const scene = filmScene(feature, [
  cardsBeat({
    seconds: 5.5,
    tag,
    cards: [
      {
        x: 240,
        y: 270,
        width: 1440,
        title: 'opencode.json',
        lines: POLICY_LINES.map((text, index) => ({
          text,
          typeAt: POLICY_TYPE_START + index * 0.9,
          charsPerSecond: POLICY_CHARS_PER_SECOND,
        })),
      },
    ],
    caption: { en: 'Allow, ask or deny, per tool and pattern.', zh: '按工具和命令模式，允许、询问或拒绝。' },
  }),
  windowBeat({
    seconds: 8,
    tag,
    title: 'ref',
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1.2, x: 0.6, y: 0.55, zoom: 1.5 },
      { at: 4, x: 0.5, y: 0.62, zoom: 2 },
    ],
    marks: [{ x: 0.31, y: 0.7, width: 0.31, height: 0.055, from: 4.4, to: 7.4 }],
    pointer: {
      path: [
        { at: 0, x: 0.78, y: 0.9 },
        { at: 4.2, x: 0.78, y: 0.9 },
        { at: 5.8, x: 0.352, y: 0.728 },
      ],
    },
    caption: [
      { from: 0.5, to: 3.6, text: { en: 'The call waits for your answer.', zh: '调用会等你的答复。' } },
      { from: 4, to: 7.6, text: { en: 'Allow once, for the session, or deny.', zh: '允许一次、允许本会话，或拒绝。' } },
    ],
  }),
  windowBeat({
    seconds: 6.5,
    tag,
    screens: [{ name: 'gui-delay-ring' }, { name: 'gui-delay-cancelled', at: 3.6 }],
    looks: [
      { at: 0, x: 0.6, y: 0.3, zoom: 1.7 },
      { at: 3, x: 0.6, y: 0.3, zoom: 1.7 },
    ],
    pointer: {
      path: [
        { at: 0, x: 0.7, y: 0.6 },
        { at: 0.8, x: 0.7, y: 0.6 },
        { at: 2.4, x: 0.753, y: 0.306 },
      ],
      clicks: [3.4],
    },
    caption: { en: 'A countdown gives you time to stop it.', zh: '倒计时让你来得及拦下。' },
  }),
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-denied' }],
    looks: [
      { at: 0, x: 0.6, y: 0.3, zoom: 1.5 },
    ],
    caption: { en: 'Denied calls do not stop the work.', zh: '被拒绝的调用不会中断工作。' },
  }),
])
