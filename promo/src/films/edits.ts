import { FEATURES } from './features.ts'
import { cardsBeat, filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.edits
const tag = tagOf(feature)

// The picture is the permission card of an `edit` call: its input names the file tag and the line.
export const scene = filmScene(feature, [
  windowBeat({
    seconds: 7,
    tag,
    screens: [{ name: 'gui-permission-card' }],
    looks: [
      { at: 0, x: 0.6, y: 0.5, zoom: 1 },
      { at: 1.2, x: 0.45, y: 0.48, zoom: 2.2 },
    ],
    marks: [
      { x: 0.364, y: 0.487, width: 0.066, height: 0.028, from: 2.4, to: 6.4 },
      { x: 0.328, y: 0.577, width: 0.076, height: 0.028, from: 3.4, to: 6.4 },
    ],
    caption: { en: 'An edit names the file tag and the lines it changes.', zh: '修改时指明文件标签和行号。' },
  }),
  windowBeat({
    seconds: 5.5,
    tag,
    screens: [{ name: 'gui-edit-diff' }],
    // The zoom keeps the diff header, which has the random mock path, out of view.
    looks: [
      { at: 0, x: 0.6, y: 0.455, zoom: 1.7 },
    ],
    marks: [{ x: 0.305, y: 0.31, width: 0.595, height: 0.052, from: 1.2, to: 5 }],
    caption: { en: 'The result is a diff you can read.', zh: '结果是一份看得懂的 diff。' },
  }),
  cardsBeat({
    seconds: 8.5,
    tag,
    cards: [
      {
        x: 260,
        y: 170,
        width: 1400,
        title: { en: 'Illustration, after docs/tools.md', zh: '示意，依据 docs/tools.md' },
        lines: [
          { text: '[src/main.rs#3F2A]', typeAt: 0.6, charsPerSecond: 40 },
          { text: '1:fn main() {', typeAt: 1.2, charsPerSecond: 40 },
          { text: '2:    println!("hi");', typeAt: 1.6, charsPerSecond: 40 },
          { text: '3:}', typeAt: 2.1, charsPerSecond: 40 },
          { text: '' },
          { text: 'edit  tag 3F2A  replace line 2', typeAt: 3, charsPerSecond: 40 },
          { text: 'src/main.rs changed since #3F2A', tone: 'bad', typeAt: 4.2, charsPerSecond: 40 },
          {
            text: { en: 'mapped onto the current file through a diff', zh: '能对上时，通过 diff 映射到当前文件' },
            tone: 'good',
            typeAt: 5.2,
            charsPerSecond: 40,
          },
          {
            text: { en: 'or rejected, with the current lines returned', zh: '对不上时，拒绝并返回当前内容' },
            tone: 'bad',
            typeAt: 6.4,
            charsPerSecond: 40,
          },
        ],
      },
    ],
    caption: [
      { from: 0.5, to: 3.4, text: { en: 'Every read comes with a tag.', zh: '每次读取都带标签。' } },
      { from: 3.6, to: 8, text: { en: 'A changed file is mapped or rejected, never overwritten blindly.', zh: '文件变了，要么映射，要么拒绝，不会盲目覆盖。' } },
    ],
  }),
])
