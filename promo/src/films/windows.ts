// C6 "One session, three windows" (shooting script section 5), mock script `todo` (M7).
// The prompt is typed in the desktop app (clip gui-todo); the terminal and the browser are
// stills taken at the clip's marks (capture/todo.ts), crossfaded when the take changes them.
import { windowOutline } from '../components/windowOutline.ts'
import { clip } from '../engine/clips.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { threeWindowsShot } from '../shots/threeWindows.ts'
import { FEATURES } from './features.ts'
import { cues, endCut, timelineFilm } from './timeline.ts'

const HOOK_END = 3.0
const IDLE_END = 7.0
const TAKE_END = 18.0
const FILM_END = 20.5

const TODO_CLIP = 'gui-todo'
// The take's cut starts this long into the clip, just before the prompt is typed.
const CLIP_FROM_SECONDS = 0.5
// Seconds after Enter at which the prompt shows everywhere and the `todo` script ticks its
// items off (TODO_FIRST_TICK_MS and TODO_NEXT_TICK_MS in crates/server/src/mock/scripts/tools.rs).
const SHOWN_AFTER_ENTER = 0.4
const TICKS_AFTER_ENTER = [4.8, 7.1, 9.4]

// Stills of the terminal and the browser at each step, in order.
const STEPS = ['sent', 'tick1', 'tick2', 'done']

function stepTimes(): number[] {
  const enter = clip(TODO_CLIP).marks.enter
  if (enter === undefined) throw new Error(`${TODO_CLIP} lacks the mark enter`)
  return [SHOWN_AFTER_ENTER, ...TICKS_AFTER_ENTER].map((after) => enter + after - CLIP_FROM_SECONDS)
}

function stills(kind: 'tui' | 'web', suffix: string, times: readonly number[]) {
  return [{ name: `${kind}-todo-idle${suffix}` }, ...STEPS.map((step, index) => ({ name: `${kind}-todo-${step}${suffix}`, at: times[index] }))]
}

// Where each window looks, and its user message (the prompt) once it shows, in picture coordinates.
const TUI_LOOK = { at: 0, x: 0.5, y: 0.5, zoom: 1.1 }
const GUI_LOOK = { at: 0, x: 0.55, y: 0.45, zoom: 1.4 }
const WEB_LOOK = { at: 0, x: 0.55, y: 0.4, zoom: 1.6 }
const TUI_PROMPT = { x: 0.03, y: 0.2, width: 0.3, height: 0.08 }
const GUI_PROMPT = { x: 0.295, y: 0.255, width: 0.17, height: 0.06 }
const WEB_PROMPT = { x: 0.34, y: 0.198, width: 0.135, height: 0.052 }
const PROMPT_MARK_DELAY = 0.3

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
          { screens: [{ name: 'tui-todo-idle-clean' }], title: 'ref', look: TUI_LOOK },
          { screens: [{ name: 'gui-todo-idle' }], title: 'ref-gui', look: GUI_LOOK },
          { screens: [{ name: 'web-todo-idle' }], title: 'ref-web', look: WEB_LOOK },
        ]),
    },
    {
      from: IDLE_END,
      to: TAKE_END,
      enter: 'cut',
      build: (seconds) => {
        const times = stepTimes()
        // The prompt is called out in all three from when it shows until the first tick.
        const promptMark = (area: typeof GUI_PROMPT) => [{ ...area, from: times[0]! + PROMPT_MARK_DELAY, to: times[1]! }]
        return threeWindowsShot(
          seconds,
          [
            { screens: stills('tui', '-clean', times), title: 'ref', look: TUI_LOOK, marks: promptMark(TUI_PROMPT) },
            { screens: [{ clip: TODO_CLIP, at: -CLIP_FROM_SECONDS }], title: 'ref-gui', look: GUI_LOOK, marks: promptMark(GUI_PROMPT) },
            { screens: stills('web', '', times), title: 'ref-web', look: WEB_LOOK, marks: promptMark(WEB_PROMPT) },
          ],
          false,
        )
      },
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
