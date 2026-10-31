// One session shown by ref (terminal), ref-gui and ref-web at the same time: all three
// connect to one mock backend and open the session the terminal started.
import { join } from 'node:path'
import type { BrowserContext } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import { isolatedEnv, startMockBackend, startMockWeb } from './lib/backend.ts'
import { GuiSession } from './lib/guiSession.ts'
import { publishImage, publishWithoutFooter } from './lib/publish.ts'
import { renderTerminalStill } from './lib/terminalStill.ts'
import { KEYS, TuiSession } from './lib/tuiSession.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const GRAB_FILE = join(import.meta.dirname, '..', '.cache', 'grab', 'gui-shared.png')
const GUI_SIZE = { width: 1280, height: 800 }
const WEB_VIEWPORT = { width: 1600, height: 1000 }
const STREAM_WAIT_MS = 6000
const SETTLE_MS = 1000
// Sessions are listed by name ("workspace-" and four hex digits, random per run); the terminal
// shows its own in the status line, and the sidebar row with that name opens it in the other two.
const SCRIPT = 'slow'
const SESSION_NAME = /workspace-[0-9a-f]{4}/
const ROW_TIMEOUT_MS = 8000
const ARTICLE_TITLE = 'A long article'

export async function captureShared(): Promise<void> {
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
    await tui.type(SCRIPT, 50)
    tui.send(KEYS.enter)
    await tui.waitForText(ARTICLE_TITLE)

    const sessionName = SESSION_NAME.exec(tui.screenText())?.[0]
    if (!sessionName) throw new Error('the terminal does not show its session name')

    await gui.waitForWindow()
    await gui.waitForText('Mock 1')
    await gui.waitForText(sessionName)
    await gui.clickText(sessionName)
    await gui.waitForText(ARTICLE_TITLE)

    context = await browser.newContext({ viewport: WEB_VIEWPORT, deviceScaleFactor: 2, locale: 'en-US', colorScheme: 'dark', permissions: ['notifications'] })
    const page = await context.newPage()
    await page.goto(web.loginUrl)
    await page.waitForSelector('textarea:not([disabled])')
    await page.getByText(sessionName, { exact: true }).first().click({ timeout: ROW_TIMEOUT_MS }).catch(async (error) => {
      await page.screenshot({ path: join(import.meta.dirname, '..', '.cache', 'failure-web.png') })
      throw error
    })
    await page.getByText(ARTICLE_TITLE).first().waitFor()

    await tui.pause(STREAM_WAIT_MS)
    await gui.pause(SETTLE_MS)
    await publishImage(await gui.grab(GRAB_FILE), 'gui-shared', { readme: false })
    await publishImage(await page.screenshot({ type: 'png' }), 'web-shared', { readme: false })
    const terminal = await renderTerminalStill(browser, { cols: TUI_COLS, rows: TUI_ROWS }, tui.outputSoFar())
    await publishImage(terminal, 'tui-shared', { readme: false })
    await publishWithoutFooter('tui-shared', { readme: false })
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
