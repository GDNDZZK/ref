// C4 "The model asks, you answer" (shooting script section 5), mock script `question-one` (M6).
import { keycap } from '../components/keycap.ts'
import { notificationCard } from '../components/notificationCard.ts'
import { textCard } from '../components/textCard.ts'
import { timeSkipChip } from '../components/timeSkipChip.ts'
import { beatShot } from '../shots/beat.ts'
import { HOOK_RECT, terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, endCut, timelineFilm, type Cut } from './timeline.ts'

const HOOK_END = 3.4
const NOTICE_END = 7.4
const ANSWER_END = 17.4
const BROWSER_END = 20.4
const FILM_END = 22.9

// Clip seconds (gui-question): the dialog is up, Enter is pressed (past the 350 ms guard).
const DIALOG_UP_CLIP_SECONDS = 2
const ENTER_CLIP_SECONDS = 6.6
const SESSION_TITLE = 'parser-fix'
const QUESTION = { en: 'Asks: Which test runner should I use?', zh: '提问：Which test runner should I use?' }

// The viewer's own editor in front, ref-gui behind it; the notification slides in over the editor.
const noticeCut: Cut = {
  from: HOOK_END,
  to: NOTICE_END,
  build: (seconds) =>
    beatShot({
      seconds,
      screens: [{ clip: 'gui-question' }],
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
        shot.add(notificationCard({ title: SESSION_TITLE, body: QUESTION, from: 0.6, to: seconds + 1 }))
      },
    }),
}

const dialogCut: Cut = {
  from: NOTICE_END,
  to: ANSWER_END,
  build: (seconds) =>
    beatShot({
      seconds,
      screens: [{ clip: 'gui-question', at: -DIALOG_UP_CLIP_SECONDS }],
      looks: [{ at: 0, x: 0.6, y: 0.6, zoom: 1.5 }],
      extras: (shot) => {
        const enterAt = ENTER_CLIP_SECONDS - DIALOG_UP_CLIP_SECONDS
        shot.add(keycap({ keys: ['Enter'], x: 960, y: 760, presses: [enterAt], from: enterAt - 0.6, to: enterAt + 1.2 }))
      },
    }),
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
        beatShot({ seconds, screens: [{ name: 'web-question-alert' }], looks: [{ at: 0, x: 0.5, y: 0.4, zoom: 1.2 }] }),
    },
    endCut(BROWSER_END, FILM_END),
  ],
  captions: cues([
    [0.2, 3.3, { en: 'It stopped to ask you. You were elsewhere.', zh: '任务停下来问你，你却在忙别的。' }],
    [3.6, 7.2, { en: 'A notification shows the question.', zh: '通知里直接带着问题。' }],
    [7.6, 10.8, { en: 'It offers options and a default.', zh: '它给出选项和默认项。' }],
    [11.2, 14.2, { en: 'Press Enter to take the default.', zh: '回车就选默认项。' }],
    [14.6, 17.2, { en: 'The task continues with your answer.', zh: '任务按你的回答继续。' }],
    [17.6, 20.2, { en: 'The browser UI notifies you too.', zh: '浏览器也会通知你。' }],
  ]),
  chip: { ...FEATURES.questions, from: HOOK_END + 0.1, to: BROWSER_END },
  overlay: (shot) =>
    shot.add(timeSkipChip({ text: '+47 min', x: HOOK_RECT.x + HOOK_RECT.width - 220, y: HOOK_RECT.y + 70, from: -1, to: HOOK_END })),
})
