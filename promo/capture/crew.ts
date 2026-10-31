// A9 and B6: the crew of the hero take started in the terminal and watched from ref-gui (split
// view) and ref-web at the same moment: one crew, three windows, on the session `main`.
import type { Browser } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import type { Lang } from '../src/engine/i18n.ts'
import { KEYS } from './lib/tuiSession.ts'
import { TOAST_MS, photograph, publishStills, withThreeClients, type ThreeClientsOptions } from './lib/threeClients.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const TASK = 'orchestrate: fix the parser bug with one worker per job' // as in the hero take
const PLAN_DONE = 'Four helpers are working'
const HELPERS_SEEN = 'docs'
// The plan ends about 7.5 s after the task, with all four helpers working and the fixer waiting for approval.
const SETTLE_MS = 800
const TYPING_DELAY_MS = 40

async function takeCrew(browser: Browser, lang: Lang): Promise<void> {
  const options: ThreeClientsOptions = { lang, session: 'main', terminal: { cols: TUI_COLS, rows: TUI_ROWS }, gui: { width: 1280, height: 1120 } }
  await withThreeClients(browser, options, async (clients) => {
    const { tui, gui, page } = clients
    await gui.pause(TOAST_MS)
    await tui.type(TASK, TYPING_DELAY_MS)
    tui.send(KEYS.enter)
    await gui.pause(400)
    await gui.command('watch') // the split view needs the session to be running
    await gui.parkPointer()
    await tui.waitForText(PLAN_DONE)
    await page.getByText(HELPERS_SEEN, { exact: true }).first().waitFor()
    await tui.pause(SETTLE_MS)
    const still = await photograph(clients, 'crew')
    await gui.shot('crew')
    await publishStills(browser, options, [still])
  })
}

export async function captureCrew(langs: readonly Lang[]): Promise<void> {
  const browser = await launchChromium()
  try {
    for (const lang of langs) {
      console.log(`crew ${lang}`)
      await takeCrew(browser, lang)
    }
  } finally {
    await browser.close()
  }
}
