// The `orchestrate` mock script started in the terminal and watched from ref-gui (split view) and
// ref-web at the same moment: one crew, three windows. Like shared.ts, all three connect to one
// mock backend and open the session the terminal started.
import { join } from 'node:path'
import type { BrowserContext } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import { isolatedEnv, startMockBackend, startMockWeb } from './lib/backend.ts'
import { GuiSession } from './lib/guiSession.ts'
import { publishImage, publishWithoutFooter } from './lib/publish.ts'
import { renderTerminalStill } from './lib/terminalStill.ts'
import { KEYS, TuiSession } from './lib/tuiSession.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const GRAB_FILE = join(import.meta.dirname, '..', '.cache', 'grab', 'gui-crew.png')
const GUI_SIZE = { width: 1280, height: 1120 }
const WEB_VIEWPORT = { width: 1600, height: 1000 }
const TASK = 'orchestrate fix the parser bug, test it, update the docs'
const SESSION_NAME = /workspace-[0-9a-f]{4}/
const ROW_TIMEOUT_MS = 8000
const PLAN_DONE = 'Four helpers are working'
const HELPERS_SEEN = 'docs'
// The plan ends about 7.5 s after the task, with all four helpers working and the fixer waiting for approval.
const SETTLE_MS = 800
const CLOSE_SESSION_KEY = 'Alt+K'
const GUI_CLOSE_SESSION_KEY = 'Ctrl+Shift+K'
const GUI_WATCH_KEY = 'Ctrl+Shift+W'
const TYPING_DELAY_MS = 50

export async function captureCrew(): Promise<void> {
  const isolated = isolatedEnv()
  const backend = await startMockBackend(isolated)
  const web = await startMockWeb(isolated, backend)
  const browser = await launchChromium()
  const tui = new TuiSession({ cols: TUI_COLS, rows: TUI_ROWS }, backend, isolated)
  let context: BrowserContext | undefined
  const gui = new GuiSession({ ...GUI_SIZE, lang: 'en', theme: 'dark' }, backend, isolated)
  try {
    await tui.waitForText('Build ·')
    await tui.pause(800)
    const sessionName = SESSION_NAME.exec(tui.screenText())?.[0]
    if (!sessionName) throw new Error('the terminal does not show its session name')

    // Both windows start with a session of their own, which would become a tile and a list row.
    await gui.waitForWindow()
    await gui.waitForText('Mock 1')
    await gui.key(GUI_CLOSE_SESSION_KEY)
    await gui.waitForText(sessionName)
    await gui.clickText(sessionName)
    await gui.key(GUI_WATCH_KEY)

    context = await browser.newContext({ viewport: WEB_VIEWPORT, deviceScaleFactor: 2, locale: 'en-US', colorScheme: 'dark', permissions: ['notifications'] })
    const page = await context.newPage()
    await page.goto(web.loginUrl)
    await page.waitForSelector('textarea:not([disabled])')
    await page.keyboard.press(CLOSE_SESSION_KEY)
    await page.getByText(sessionName, { exact: true }).first().click({ timeout: ROW_TIMEOUT_MS })

    await tui.type(TASK, TYPING_DELAY_MS)
    tui.send(KEYS.enter)
    await tui.waitForText(PLAN_DONE)
    await page.getByText(HELPERS_SEEN, { exact: true }).first().waitFor()
    await tui.pause(SETTLE_MS)

    await publishImage(await gui.grab(GRAB_FILE), 'gui-crew', { readme: false, sessionName })
    await publishImage(await page.screenshot({ type: 'png' }), 'web-crew', { readme: false, sessionName })
    const terminal = await renderTerminalStill(browser, { cols: TUI_COLS, rows: TUI_ROWS }, tui.outputSoFar())
    await publishImage(terminal, 'tui-crew', { readme: false })
    await publishWithoutFooter('tui-crew', { readme: false })
  } finally {
    await context?.close() // open pages keep ref-web from stopping
    await gui.close()
    await tui.close()
    await browser.close()
    await web.stop()
    await backend.stop()
    isolated.remove()
  }
}
