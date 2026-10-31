// M7 in the shooting script, for C6: ref (terminal), ref-gui and ref-web on one session `main`
// of one mock backend. The prompt is typed in ref-gui, which records a clip; the terminal and
// the browser are photographed at the clip's marks, so the films can switch their stills there.
import type { Browser } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import type { Lang } from '../src/engine/i18n.ts'
import { TOAST_MS, clock, photograph, publishStills, withThreeClients, type ThreeClientsOptions } from './lib/threeClients.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const PROMPT = 'todo list for the parser fix'
const PLAN_SHOWN = 'Read the README' // the first todo item
const TYPING_SECONDS = 1.0
const BEFORE_TYPING_MS = 600
// Seconds after Enter at which the terminal and the browser are photographed. The `todo` script
// ticks its items off about 4.8, 7.1 and 9.4 s after the prompt (TODO_FIRST_TICK_MS and
// TODO_NEXT_TICK_MS in crates/server/src/mock/scripts/tools.rs).
const MOMENTS: [string, number][] = [
  ['sent', 1.0],
  ['tick1', 5.4],
  ['tick2', 7.7],
  ['done', 10.0],
]
const CLIP_END_SECONDS = 11.5

async function takeTodo(browser: Browser, lang: Lang): Promise<void> {
  const options: ThreeClientsOptions = { lang, session: 'main', terminal: { cols: TUI_COLS, rows: TUI_ROWS }, gui: { width: 1280, height: 800 } }
  await withThreeClients(browser, options, async (clients) => {
    const { gui } = clients
    await gui.pause(TOAST_MS)
    const stills = [await photograph(clients, 'todo-idle')]
    await gui.shot('todo-idle')

    await gui.startClip('todo')
    await gui.pause(BEFORE_TYPING_MS)
    for (const character of PROMPT) {
      await gui.type(character)
      await gui.pause((TYPING_SECONDS * 1000) / PROMPT.length)
    }
    await gui.key('Enter')
    gui.markClip('enter')
    const time = clock(gui)
    for (const [mark, seconds] of MOMENTS) {
      await time.until(seconds)
      gui.markClip(mark)
      stills.push(await photograph(clients, `todo-${mark}`))
    }
    if (!(await gui.hasText(PLAN_SHOWN))) throw new Error('the todo list never showed in the GUI')
    await time.until(CLIP_END_SECONDS)
    await gui.endClip()
    await publishStills(browser, options, stills)
  })
}

export async function captureTodo(langs: readonly Lang[]): Promise<void> {
  const browser = await launchChromium()
  try {
    for (const lang of langs) {
      console.log(`todo ${lang}`)
      await takeTodo(browser, lang)
    }
  } finally {
    await browser.close()
  }
}
