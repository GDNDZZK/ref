// C6 "One session, three windows" (shooting script section 5), mock script `todo` (M7).
// The browser is shown as stills crossfaded at the moments of the take (no browser clip).
import { windowOutline } from '../components/windowOutline.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { threeWindowsShot } from '../shots/threeWindows.ts'
import { FEATURES } from './features.ts'
import { cues, endCut, timelineFilm } from './timeline.ts'

const HOOK_END = 3.0
const IDLE_END = 7.0
const TAKE_END = 18.0
const FILM_END = 20.5

// Seconds into the take's cut (from film 7.0): the prompt shows everywhere, then the todo items tick.
const SENT_AT = 2.6
const TICKED_AT = 8.0

const HOOK_TERMINAL_RECT = { x: 80, y: 120, width: 1060, height: 700 }

export const scene = timelineFilm({
  id: 'windows',
  cuts: [
    terminalCut(0, HOOK_END, {
      rect: HOOK_TERMINAL_RECT,
      tabs: [{ label: 'devbox', active: true, spinner: true }],
      lines: [
        { at: -1, prompt: '$ ', text: 'agent "fix the parser and run the tests"' },
        { at: -1, text: '● Read src/parser.rs', tone: 'accent' },
        { at: 0.6, text: '● Edit src/parser.rs', tone: 'accent' },
        { at: 1.4, text: '● Bash cargo test', tone: 'accent' },
      ],
      extras: (shot) => shot.add(windowOutline({ rect: { x: 1180, y: 120, width: 660, height: 700 }, from: -1, to: HOOK_END + 1 })),
    }),
    {
      from: HOOK_END,
      to: IDLE_END,
      build: (seconds) =>
        threeWindowsShot(seconds, [
          { screens: [{ name: 'tui-todo-idle' }], title: 'ref', look: { at: 0, x: 0.5, y: 0.5, zoom: 1 } },
          { screens: [{ name: 'gui-todo-idle' }], title: 'ref-gui', look: { at: 0, x: 0.6, y: 0.5, zoom: 1.2 } },
          { screens: [{ name: 'web-todo-idle' }], title: 'ref-web', look: { at: 0, x: 0.6, y: 0.5, zoom: 1.2 } },
        ]),
    },
    {
      from: IDLE_END,
      to: TAKE_END,
      enter: 'cut',
      build: (seconds) =>
        threeWindowsShot(
          seconds,
          [
            {
              screens: [{ name: 'tui-todo-idle' }, { name: 'tui-todo-sent', at: SENT_AT }, { name: 'tui-todo-done', at: TICKED_AT }],
              title: 'ref',
              look: { at: 0, x: 0.5, y: 0.5, zoom: 1 },
            },
            { screens: [{ clip: 'gui-todo' }], title: 'ref-gui', look: { at: 0, x: 0.6, y: 0.5, zoom: 1.2 } },
            {
              screens: [{ name: 'web-todo-idle' }, { name: 'web-todo-sent', at: SENT_AT }, { name: 'web-todo-done', at: TICKED_AT }],
              title: 'ref-web',
              look: { at: 0, x: 0.6, y: 0.5, zoom: 1.2 },
            },
          ],
          false,
        ),
    },
    endCut(TAKE_END, FILM_END),
  ],
  captions: cues([
    [0.2, 2.9, { en: 'Started in one window, needed in another.', zh: '在一个窗口开始，要在另一个窗口看。' }],
    [3.2, 6.8, { en: 'One session, open in three windows.', zh: '同一个会话，三个窗口。' }],
    [7.2, 11.0, { en: 'Type in the desktop app. All three show it.', zh: '在桌面端输入，三个窗口同步出现。' }],
    [13.2, 17.8, { en: 'Todo items tick off in every window.', zh: '待办一项项在三个窗口里打勾。' }],
  ]),
  chip: { ...FEATURES.windows, from: HOOK_END + 0.1, to: TAKE_END },
})
