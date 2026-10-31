// C4 "The model asks, you answer" (shooting script section 5), mock script `question-one` (M6).
import { browserTab } from '../components/browserTab.ts'
import { keycap } from '../components/keycap.ts'
import { notificationCard } from '../components/notificationCard.ts'
import { textCard } from '../components/textCard.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { clip } from '../engine/clips.ts'
import { beatShot } from '../shots/beat.ts'
import { HOOK_RECT, terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, endCut, timelineFilm, type Cut } from './timeline.ts'

const HOOK_END = 3.4
const NOTICE_END = 7.4
const ENTER_AT = 12.0
const ANSWER_END = 17.4
const BROWSER_END = 20.4
const FILM_END = 22.9

// gui-question marks: the dialog is up, Enter is pressed (capture/scenarios.ts `question`).
const QUESTION_CLIP = 'gui-question'
const NOTICE_AT = 0.6 // seconds into the notice cut at which the notification slides in
const DIALOG_SETTLED_SECONDS = 0.2 // the dialog's entrance
const SESSION_TITLE = 'parser-fix'
const QUESTION = { en: 'Asks: Which file should I change first?', zh: '提问：Which file should I change first?' }
// The default option, in picture coordinates, and where the camera looks.
const DEFAULT_OPTION = { x: 0.305, y: 0.51, width: 0.592, height: 0.072 }
const DIALOG_LOOK = { x: 0.6, y: 0.6, zoom: 1.5 }
const REPLY_LOOK = { x: 0.55, y: 0.32, zoom: 1.6 }
const REPLY_LOOK_DELAY = 0.3
const REPLY_LOOK_MOVE = 0.8
const WEB_DIALOG_LOOK = { x: 0.58, y: 0.3, zoom: 2 }
const WEB_TAB_TITLE = `${SESSION_TITLE} · ref` // the page title (web/src/App.vue)

function questionMarks() {
  const { marks } = clip(QUESTION_CLIP)
  if (marks.dialog === undefined || marks.answer === undefined) throw new Error(`${QUESTION_CLIP} lacks the marks dialog and answer`)
  return { dialog: marks.dialog, answer: marks.answer }
}

// The viewer's own editor in front, ref-gui behind it; the notification slides in over the editor.
const noticeCut: Cut = {
  from: HOOK_END,
  to: NOTICE_END,
  build: (seconds) =>
    beatShot({
      seconds,
      // The notification comes when the question does.
      screens: [{ clip: QUESTION_CLIP, at: NOTICE_AT - questionMarks().dialog }],
      looks: [{ at: 0, x: 0.5, y: 0.5, zoom: 1 }],
      pointer: { path: [{ at: 0.8, x: 0.6, y: 0.6 }, { at: 2.4, x: 0.98, y: -0.02 }], clicks: [2.6] },
      extras: (shot) => {
        shot.add(
          textCard({
            x: 420,
            y: 240,
            width: 1080,
            title: 'parser.rs',
            lines: [
              { text: 'pub fn split_fields(line: &str) -> Vec<&str> {' },
              { text: '    line.split(\',\').map(str::trim).collect()' },
              { text: '}' },
              { text: '' },
              { text: '#[cfg(test)]' },
              { text: 'mod tests {' },
            ],
            from: 0,
            to: 2.9,
          }),
        )
        shot.add(notificationCard({ title: SESSION_TITLE, body: QUESTION, from: NOTICE_AT, to: seconds + 1 }))
      },
    }),
}

// The session opens on the dialog; Enter takes the default at ENTER_AT and the reply follows.
const dialogCut: Cut = {
  from: NOTICE_END,
  to: ANSWER_END,
  build: (seconds) => {
    const marks = questionMarks()
    const enterAt = ENTER_AT - NOTICE_END
    const clipOffset = marks.answer - enterAt
    const firstClipSeconds = marks.dialog + DIALOG_SETTLED_SECONDS
    return beatShot({
      seconds,
      screens: [{ clip: QUESTION_CLIP, time: (time) => Math.max(firstClipSeconds, time + clipOffset) }],
      looks: [
        { at: 0, ...DIALOG_LOOK },
        // The dialog closes on Enter and the reply is right there.
        { at: enterAt + REPLY_LOOK_DELAY, ...DIALOG_LOOK },
        { at: enterAt + REPLY_LOOK_DELAY + REPLY_LOOK_MOVE, ...REPLY_LOOK, ease: 'inOutSine' },
      ],
      marks: [{ ...DEFAULT_OPTION, from: 0.6, to: enterAt }],
      extras: (shot) => {
        shot.add(keycap({ keys: ['Enter'], x: 960, y: 760, presses: [enterAt], from: enterAt - 0.6, to: enterAt + 1.2 }))
      },
    })
  },
}

export const scene = timelineFilm({
  id: 'question',
  cuts: [
    terminalCut(0, HOOK_END, {
      lines: [
        { at: -1, prompt: '$ ', text: 'agent "add tests for the parser"' },
        { at: -1, text: '' },
        { at: -1, text: 'Waiting for input...', tone: 'warn' },
      ],
    }),
    noticeCut,
    dialogCut,
    {
      from: ANSWER_END,
      to: BROWSER_END,
      build: (seconds) =>
        beatShot({
          seconds,
          screens: [{ name: 'web-question-alert' }],
          looks: [{ at: 0, ...WEB_DIALOG_LOOK }],
          title: 'ref-web',
          extras: (shot) => shot.add(browserTab({ title: WEB_TAB_TITLE, alertAt: 0.5, from: 0, to: seconds + 1 })),
        }),
    },
    endCut(BROWSER_END, FILM_END),
  ],
  captions: cues([
    [0.2, 3.3, { en: 'It stopped to ask you. You were elsewhere.', zh: '任务停下来问你，你却在忙别的。' }],
    [3.6, 7.2, { en: 'A notification shows the question.', zh: '通知里直接带着问题。' }],
    [7.6, 10.8, { en: 'It offers options and a default.', zh: '它给出选项和默认项。' }],
    [11.2, 14.2, { en: 'Press Enter to take the default.', zh: '回车就选默认项。' }],
    [14.6, 17.2, { en: 'The model gets your answer.', zh: '模型拿到了你的回答。' }],
    [17.6, 20.2, { en: 'The browser UI notifies you too.', zh: '浏览器也会通知你。' }],
  ]),
  chip: { ...FEATURES.questions, from: HOOK_END + 0.1, to: BROWSER_END },
  overlay: (shot) =>
    shot.add(timeSkipChip({ text: '+47 min', x: HOOK_RECT.x + HOOK_RECT.width - 220, y: HOOK_RECT.y + 70, from: -1, to: HOOK_END })),
})
