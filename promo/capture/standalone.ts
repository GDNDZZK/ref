// M2 in the shooting script, for Film A A10: ref, ref-gui and ref-web on `main` while the
// `orchestrate-long` crew works. The prompt is sent from the terminal, the fixer's edit is allowed
// in the GUI's split view at 10 s, and all three are photographed at 20 s, the moment the film
// closes the windows. The uptime and the running sessions at that moment are printed for the film.
import type { Browser } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import type { Lang } from '../src/engine/i18n.ts'
import { KEYS } from './lib/tuiSession.ts'
import { TOAST_MS, clock, photograph, publishStills, withThreeClients, type ThreeClientsOptions } from './lib/threeClients.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const PROMPT = 'orchestrate-long: fix the parser bug with one worker per job'
const APPROVAL_ASKED_FOR = 'Fix the off-by-one' // the fixer's edit
const TUI_TYPING_DELAY_MS = 40
const WATCH_AT = 0.4
const ALLOW_AT = 10
const CLOSE_AT = 20
const GUI_SIZE = { width: 1280, height: 1120 } // the split view of the hero take

async function takeStandalone(browser: Browser, lang: Lang): Promise<void> {
  const options: ThreeClientsOptions = { lang, session: 'main', terminal: { cols: TUI_COLS, rows: TUI_ROWS }, gui: GUI_SIZE }
  await withThreeClients(browser, options, async (clients) => {
    const { tui, gui, page } = clients
    await gui.pause(TOAST_MS)
    await tui.type(PROMPT, TUI_TYPING_DELAY_MS)
    tui.send(KEYS.enter)
    const time = clock(gui)
    await time.until(WATCH_AT)
    await gui.command('watch')
    await gui.parkPointer()
    await time.until(ALLOW_AT)
    await gui.clickTextBelow(gui.text('tool.allow'), APPROVAL_ASKED_FOR)
    await gui.parkPointer()
    await time.until(CLOSE_AT)
    const still = await photograph(clients, 'standalone')
    await gui.shot('standalone')
    console.log(`uptime at close: ${((Date.now() - clients.backendStartedAt) / 1000).toFixed(1)} s`)
    console.log(`browser sidebar at close:\n${await page.locator('aside').first().innerText()}`)
    await publishStills(browser, options, [still])
  })
}

export async function captureStandalone(langs: readonly Lang[]): Promise<void> {
  const browser = await launchChromium()
  try {
    for (const lang of langs) {
      console.log(`standalone ${lang}`)
      await takeStandalone(browser, lang)
    }
  } finally {
    await browser.close()
  }
}
