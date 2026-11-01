// A9 and B6: the crew of the hero take started in the terminal and watched from ref-gui (split
// view) and ref-web: one crew, three windows, on the session `main`, photographed after main's
// summary, when its to-do list reads 5/5. The fixer's edit is allowed in the GUI.
import type { Browser } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import type { Lang } from '../src/engine/i18n.ts'
import { KEYS } from './lib/tuiSession.ts'
import { TOAST_MS, clock, photograph, publishStills, withThreeClients, type ThreeClientsOptions } from './lib/threeClients.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const TASK = 'orchestrate: fix the parser bug with one worker per job' // as in the hero take
const PLAN_DONE = 'Four helpers are working'
const HELPERS_SEEN = 'docs'
const APPROVAL_ASKED_FOR = 'Fix the off-by-one' // the fixer's edit
// The last line of main's summary (about 25 s after the task): every to-do of main is done.
const SUMMARY_DONE = 'The parser bug is fixed.'
const SUMMARY_TIMEOUT_MS = 60_000
// Seconds after the task: the split view has settled (as in the standalone take).
const ALLOW_AT = 10
const SETTLE_MS = 800
const TYPING_DELAY_MS = 40
const MAIN_SESSION = 'main'

async function takeCrew(browser: Browser, lang: Lang): Promise<void> {
  const options: ThreeClientsOptions = { lang, session: 'main', terminal: { cols: TUI_COLS, rows: TUI_ROWS }, gui: { width: 1280, height: 1120 } }
  await withThreeClients(browser, options, async (clients) => {
    const { tui, gui, page } = clients
    await gui.pause(TOAST_MS)
    await tui.type(TASK, TYPING_DELAY_MS)
    tui.send(KEYS.enter)
    const time = clock(gui)
    await gui.pause(400)
    await gui.command('watch') // the split view needs the session to be running
    await gui.parkPointer()
    await tui.waitForText(PLAN_DONE)
    await page.getByText(HELPERS_SEEN, { exact: true }).first().waitFor()
    await time.until(ALLOW_AT)
    await gui.clickTextBelow(gui.text('tool.allow'), APPROVAL_ASKED_FOR)
    await gui.parkPointer()
    await tui.waitForText(SUMMARY_DONE, SUMMARY_TIMEOUT_MS)
    // The GUI sidebar's Todo section follows the focused tile, which the Allow click made fixer's.
    await gui.clickTile(MAIN_SESSION)
    await gui.parkPointer()
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
