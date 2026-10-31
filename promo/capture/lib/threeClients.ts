// ref (terminal), ref-gui and ref-web connected to one mock backend and opened on one session,
// which the terminal starts and names. Used by the takes that show the three windows together.
import type { Browser, Page } from 'playwright-core'
import type { Lang } from '../../src/engine/i18n.ts'
import { isolatedEnv, startMockBackend, startMockWeb } from './backend.ts'
import { GuiSession } from './guiSession.ts'
import { GuiUi } from './guiUi.ts'
import { publishImage, publishWithoutFooter } from './publish.ts'
import { renderTerminalStill } from './terminalStill.ts'
import { KEYS, TuiSession } from './tuiSession.ts'
import { COMMAND_LETTERS, shotFileName } from './ui.ts'

const WARM_UP = 'hello' // the session exists on the backend once it has a turn
export const WARM_UP_REPLY = 'Hello! This is the mock backend'
const TUI_TYPING_DELAY_MS = 50
const WEB_VIEWPORT = { width: 1600, height: 1000 }
const BROWSER_LOCALE: Record<Lang, string> = { en: 'en-US', zh: 'zh-CN' }
const SETTLE_MS = 1000
export const TOAST_MS = 4000 // the terminal's "Renamed" notice goes away

export interface ThreeClients {
  tui: TuiSession
  gui: GuiUi
  page: Page
  // When the backend started (Date.now()), for the uptime the films show.
  backendStartedAt: number
}

export interface ThreeClientsOptions {
  lang: Lang
  session: string
  terminal: { cols: number; rows: number }
  gui: { width: number; height: number }
}

// The terminal starts the session with a warm-up turn and names it; the GUI and the browser
// close the empty session each starts on, so the lists show only this one, and open it.
async function openEverywhere(clients: ThreeClients, session: string): Promise<void> {
  const { tui, gui, page } = clients
  await tui.waitForText('Build ·')
  await tui.pause(800)
  await tui.type(WARM_UP, TUI_TYPING_DELAY_MS)
  tui.send(KEYS.enter)
  await tui.waitForText(WARM_UP_REPLY)
  await tui.type(`/rename ${session}`, TUI_TYPING_DELAY_MS)
  tui.send(KEYS.enter)
  await tui.pause(SETTLE_MS)

  await gui.waitText(session)
  await gui.command('close')
  await gui.pause(SETTLE_MS)
  await gui.clickSidebarRow(session)
  await gui.waitText(WARM_UP_REPLY)
  await gui.parkPointer()

  await page.keyboard.press(`Alt+${COMMAND_LETTERS.close}`) // the web binds commands to Alt
  await page.waitForTimeout(SETTLE_MS)
  await page.getByText(session, { exact: true }).first().click()
  await page.getByText(WARM_UP_REPLY).first().waitFor()
}

export async function withThreeClients(browser: Browser, options: ThreeClientsOptions, body: (clients: ThreeClients) => Promise<void>): Promise<void> {
  const isolated = isolatedEnv()
  const backendStartedAt = Date.now()
  const backend = await startMockBackend(isolated)
  const web = await startMockWeb(isolated, backend)
  const tui = new TuiSession(options.terminal, backend, isolated)
  const session = new GuiSession({ ...options.gui, lang: options.lang, theme: 'dark' }, backend, isolated)
  const context = await browser.newContext({
    viewport: WEB_VIEWPORT,
    deviceScaleFactor: 2,
    locale: BROWSER_LOCALE[options.lang],
    colorScheme: 'dark',
    permissions: ['notifications'],
  })
  try {
    await session.waitForWindow()
    await session.waitForText('Mock 1')
    const page = await context.newPage()
    await page.goto(web.loginUrl)
    await page.waitForSelector('textarea:not([disabled])')
    const clients = { tui, gui: new GuiUi(session, options.lang), page, backendStartedAt }
    await openEverywhere(clients, options.session)
    await body(clients)
  } finally {
    await context.close() // open pages keep ref-web from stopping
    await session.close()
    await tui.close()
    await web.stop()
    await backend.stop()
    isolated.remove()
  }
}

// A moment of a take in the terminal and the browser; the terminal's output is rendered later.
export interface Still {
  name: string
  web: Buffer
  terminal: string
}

export async function photograph(clients: ThreeClients, name: string): Promise<Still> {
  return { name, web: await clients.page.screenshot({ type: 'png' }), terminal: clients.tui.outputSoFar() }
}

// Saves the browser stills as web-<name>[-zh] and, in English (the terminal has one language),
// the terminal stills as tui-<name> plus the -clean copy without the status line.
export async function publishStills(browser: Browser, options: ThreeClientsOptions, stills: readonly Still[]): Promise<void> {
  for (const still of stills) {
    await publishImage(still.web, shotFileName('web', still.name, options.lang), { readme: false, sessionName: options.session })
    if (options.lang !== 'en') continue
    const name = `tui-${still.name}`
    await publishImage(await renderTerminalStill(browser, options.terminal, still.terminal), name, { readme: false })
    await publishWithoutFooter(name, { readme: false })
  }
}

// Seconds since now, as a pause that returns at that moment.
export function clock(gui: GuiUi) {
  const start = Date.now()
  return {
    until: (seconds: number) => gui.pause(Math.max(0, start + seconds * 1000 - Date.now())),
    now: () => (Date.now() - start) / 1000,
  }
}
