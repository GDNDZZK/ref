// C5 "Allow for the session, a delay, rules" (shooting script section 5), mocks `permission`,
// `delay` and `denied`.
import { textCard } from '../components/textCard.ts'
import { beatShot } from '../shots/beat.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, drawnCut, endCut, timelineFilm } from './timeline.ts'

const HOOK_END = 3.0
const RULES_END = 7.5
const ALLOW_END = 13.0
const DELAY_END = 20.0
const DENIED_END = 23.0
const FILM_END = 25.5

const ASKED_SO_FAR = 6
const ASKS_IN_TOTAL = 31
const ANSWER_EVERY_SECONDS = 0.42

// Every call asks; six answered in a row, the counter says how many are left.
const askLines = Array.from({ length: ASKED_SO_FAR }, (_, index) => ({
  at: index === 0 ? -1 : index * ANSWER_EVERY_SECONDS,
  text: 'Allow edit src/parser.rs? [y/n] y',
  tone: 'warn' as const,
}))

const RULES = [
  '{',
  '  "permission": {',
  '    "bash": { "*": "ask", "git status*": "allow", "rm *": "deny" },',
  '    "edit": { "*": "ask", "*.lock": "deny" }',
  '  },',
  '  "delay": { "delete": 5 }',
  '}',
]

export const scene = timelineFilm({
  id: 'permissions',
  cuts: [
    terminalCut(0, HOOK_END, {
      lines: askLines,
      status: (time) => {
        const answered = askLines.filter((line) => time >= line.at).length
        return `${answered} of ${ASKS_IN_TOTAL}`
      },
    }),
    drawnCut(HOOK_END, RULES_END, (shot, seconds) =>
      shot.add(
        textCard({
          x: 240,
          y: 200,
          width: 1440,
          title: 'opencode.json',
          lines: RULES.map((text, index) => ({ text, typeAt: 0.3 + index * 0.45, charsPerSecond: 90 })),
          from: 0,
          to: seconds + 1,
        }),
      ),
    ),
    {
      from: RULES_END,
      to: ALLOW_END,
      build: (seconds) =>
        beatShot({
          seconds,
          screens: [{ clip: 'gui-permission' }],
          looks: [{ at: 0, x: 0.6, y: 0.7, zoom: 1.5 }],
        }),
    },
    {
      from: ALLOW_END,
      to: DELAY_END,
      build: (seconds) =>
        beatShot({ seconds, screens: [{ clip: 'gui-delay' }], looks: [{ at: 0, x: 0.6, y: 0.7, zoom: 1.5 }] }),
    },
    {
      from: DELAY_END,
      to: DENIED_END,
      build: (seconds) =>
        beatShot({ seconds, screens: [{ name: 'gui-denied' }], looks: [{ at: 0, x: 0.6, y: 0.6, zoom: 1.4 }] }),
    },
    endCut(DENIED_END, FILM_END),
  ],
  captions: cues([
    [0.2, 2.9, { en: 'Every single call asks for a yes.', zh: '每个调用都要你点一次允许。' }],
    [3.2, 7.3, { en: 'Rules per tool and per command.', zh: '按工具、按命令写规则。' }],
    [7.7, 10.9, { en: 'Allow once, or for the session.', zh: '允许一次，或本会话内一直允许。' }],
    [13.2, 16.8, { en: 'With a delay set, a delete waits five seconds.', zh: '设了延迟，删除前先等五秒。' }],
    [17.1, 19.9, { en: 'Cancel stops it before it runs.', zh: '点取消，它就不会执行。' }],
    [20.2, 22.9, { en: 'A denied call does not end the turn.', zh: '被拒绝的调用不会中断任务。' }],
  ]),
  chip: { ...FEATURES.permissions, from: HOOK_END + 0.1, to: DENIED_END },
})
