// M7 in the shooting script, for C6: ref (terminal), ref-gui and ref-web on one session `main`
// of one mock backend. The prompt is typed in ref-gui, which records a clip; the terminal and
// the browser are photographed at the clip's marks, so the films can switch their stills there.
import type { Browser, Page } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import type { Lang } from '../src/engine/i18n.ts'
import { isolatedEnv, startMockBackend, startMockWeb, type Backend } from './lib/backend.ts'
import { GuiSession } from './lib/guiSession.ts'
import { GuiUi } from './lib/guiUi.ts'
import { publishImage, publishWithoutFooter } from './lib/publish.ts'
import { renderTerminalStill } from './lib/terminalStill.ts'
import { KEYS, TuiSession } from './lib/tuiSession.ts'
import { COMMAND_LETTERS, shotFileName } from './lib/ui.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const SESSION = 'main'
const WARM_UP = 'hello' // the session exists on the backend once it has a turn
const WARM_UP_REPLY = 'Hello! This is the mock backend'
const PROMPT = 'todo list for the parser fix'
const PLAN_SHOWN = 'Read the README' // the first todo item
const TUI_TYPING_DELAY_MS = 50
const GUI_SIZE = { width: 1280, height: 800 }
const WEB_VIEWPORT = { width: 1600, height: 1000 }
const BROWSER_LOCALE: Record<Lang, string> = { en: 'en-US', zh: 'zh-CN' }
const SETTLE_MS = 1000
const TOAST_MS = 4000 // the terminal's "Renamed" notice goes away
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

interface Still {
  name: string
  web: Buffer
  terminal: string // the terminal's output so far, rendered after the take
}

// Seconds since `start`, as a pause that returns at that moment.
function clock(gui: GuiUi) {
  const start = Date.now()
  return { until: (seconds: number) => gui.pause(Math.max(0, start + seconds * 1000 - Date.now())) }
}

// The terminal starts the session and names it; the GUI and the browser open it from their lists.
async function openEverywhere(tui: TuiSession, gui: GuiUi, page: Page): Promise<void> {
  await tui.waitForText('Build ·')
  await tui.pause(800)
  await tui.type(WARM_UP, TUI_TYPING_DELAY_MS)
  tui.send(KEYS.enter)
  await tui.waitForText(WARM_UP_REPLY)
  await tui.type(`/rename ${SESSION}`, TUI_TYPING_DELAY_MS)
  tui.send(KEYS.enter)
  await tui.pause(SETTLE_MS)

  // ref-gui and ref-web each start on a new empty session; closing them keeps the lists to `main`.
  await gui.waitText(SESSION)
  await gui.command('close')
  await gui.pause(SETTLE_MS)
  await gui.clickSidebarRow(SESSION)
  await gui.waitText(WARM_UP_REPLY)
  await gui.parkPointer()

  await page.keyboard.press(`Alt+${COMMAND_LETTERS.close}`) // the web binds commands to Alt
  await page.waitForTimeout(SETTLE_MS)
  await page.getByText(SESSION, { exact: true }).first().click()
  await page.getByText(WARM_UP_REPLY).first().waitFor()
}

async function takeTodo(browser: Browser, lang: Lang): Promise<void> {
  const isolated = isolatedEnv()
  const backend: Backend = await startMockBackend(isolated)
  const web = await startMockWeb(isolated, backend)
  const tui = new TuiSession({ cols: TUI_COLS, rows: TUI_ROWS }, backend, isolated)
  const session = new GuiSession({ ...GUI_SIZE, lang, theme: 'dark' }, backend, isolated)
  const context = await browser.newContext({
    viewport: WEB_VIEWPORT,
    deviceScaleFactor: 2,
    locale: BROWSER_LOCALE[lang],
    colorScheme: 'dark',
    permissions: ['notifications'],
  })
  try {
    await session.waitForWindow()
    await session.waitForText('Mock 1')
    const gui = new GuiUi(session, lang)
    const page = await context.newPage()
    await page.goto(web.loginUrl)
    await page.waitForSelector('textarea:not([disabled])')
    await openEverywhere(tui, gui, page)
    await gui.pause(TOAST_MS)

    const stills: Still[] = [{ name: 'todo-idle', web: await page.screenshot({ type: 'png' }), terminal: tui.outputSoFar() }]
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
      stills.push({ name: `todo-${mark}`, web: await page.screenshot({ type: 'png' }), terminal: tui.outputSoFar() })
    }
    if (!(await gui.hasText(PLAN_SHOWN))) throw new Error('the todo list never showed in the GUI')
    await time.until(CLIP_END_SECONDS)
    await gui.endClip()

    for (const still of stills) {
      await publishImage(still.web, shotFileName('web', still.name, lang), { readme: false, sessionName: SESSION })
      if (lang !== 'en') continue // the terminal has one language
      const name = `tui-${still.name}`
      await publishImage(await renderTerminalStill(browser, { cols: TUI_COLS, rows: TUI_ROWS }, still.terminal), name, { readme: false })
      await publishWithoutFooter(name, { readme: false })
    }
  } finally {
    await context.close() // open pages keep ref-web from stopping
    await session.close()
    await tui.close()
    await web.stop()
    await backend.stop()
    isolated.remove()
  }
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
