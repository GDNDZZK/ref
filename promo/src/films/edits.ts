// C2 "Edits go by line number, with a file tag" (shooting script section 5), mock script `stale` (M4).
import { beatShot } from '../shots/beat.ts'
import { terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, endCut, timelineFilm } from './timeline.ts'

const HOOK_END = 3.0
const TAKE_END = 22.5
const FILM_END = 25.0
// Seconds into the recording at which shot 2 starts (the read result is on screen).
const READ_SHOWN_CLIP_SECONDS = 1.5

export const scene = timelineFilm({
  id: 'edits',
  cuts: [
    terminalCut(0, HOOK_END, {
      lines: [
        { at: -1, prompt: '$ ', text: 'git pull' },
        { at: -1, text: ' scratch/src/parser.rs | 3 +++', tone: 'muted' },
        { at: -1, text: ' 1 file changed, 3 insertions(+)', tone: 'muted' },
        { at: 0.5, text: '' },
        { at: 0.5, text: '● edit scratch/src/parser.rs, line 12', tone: 'accent' },
        { at: 1.0, text: '  read:    fn parse_line(input: &str) -> Row {', tone: 'muted' },
        { at: 1.5, text: '  line 12: // Fields are split on commas.', tone: 'error' },
      ],
    }),
    {
      from: HOOK_END,
      to: TAKE_END,
      build: (seconds) =>
        beatShot({
          seconds,
          screens: [{ clip: 'gui-stale', at: -READ_SHOWN_CLIP_SECONDS }],
          looks: [{ at: 0, x: 0.6, y: 0.5, zoom: 1.4 }],
        }),
    },
    endCut(TAKE_END, FILM_END),
  ],
  captions: cues([
    [0.2, 2.9, { en: 'Line 12 is no longer the line you read.', zh: '第 12 行已经不是你读到的那一行了。' }],
    [3.2, 6.8, { en: 'ref edits by line. Every read returns a tag.', zh: 'ref 按行号改，读文件都带标签。' }],
    [7.2, 10.3, { en: 'Then the file changes on disk.', zh: '然后磁盘上的文件被改了。' }],
    [10.7, 15.8, { en: 'A shifted file: ref moves the edit.', zh: '文件移位了，ref 会把修改对过去。' }],
    [16.2, 19.4, { en: 'If those lines changed, ref rejects the edit.', zh: '改的那几行变了，ref 就拒绝。' }],
    [19.7, 22.4, { en: 'The model retries with the current lines.', zh: '模型按最新内容重试。' }],
  ]),
  chip: { ...FEATURES.edits, from: HOOK_END + 0.1, to: TAKE_END },
})
