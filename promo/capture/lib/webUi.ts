import { join } from 'node:path'
import type { Browser, Page } from 'playwright-core'
import type { Lang } from '../../src/engine/i18n.ts'
import { startMockWeb, isolatedEnv } from './backend.ts'
import { phrase } from './phrases.ts'
import { publishImage } from './publish.ts'
import { COMMAND_LETTERS, shotFileName, type CommandId, type ShotOptions, type Ui } from './ui.ts'

const VIEWPORT = { width: 1600, height: 1000 }
const DEVICE_SCALE_FACTOR = 2 // crisp when a video zooms in
const FAILURE_SHOT = join(import.meta.dirname, '..', '..', '.cache', 'failure-web.png')
const TIMEOUT_MS = 20_000
const SETTLE_MS = 700 // entrance animations of the page last up to 480 ms
const BROWSER_LOCALE: Record<Lang, string> = { en: 'en-US', zh: 'zh-CN' }

function playwrightChord(chord: string): string {
  return chord.replace('Ctrl', 'Control')
}

class WebUi implements Ui {
  readonly kind = 'web'
  readonly lang: Lang
  private readonly page: Page

  constructor(page: Page, lang: Lang) {
    this.page = page
    this.lang = lang
  }

  private prompt() {
    return this.page.locator('textarea')
  }

  text(key: string): string {
    return phrase(this.lang, key)
  }

  async send(text: string): Promise<void> {
    await this.prompt().fill(text)
    await this.page.keyboard.press('Enter')
  }

  async type(text: string): Promise<void> {
    await this.prompt().click()
    await this.page.keyboard.type(text)
  }

  async clearPrompt(): Promise<void> {
    await this.prompt().fill('')
  }

  async key(chord: string): Promise<void> {
    await this.page.keyboard.press(playwrightChord(chord))
  }

  async command(id: CommandId): Promise<void> {
    await this.page.keyboard.press(`Alt+${COMMAND_LETTERS[id]}`)
  }

  async clickText(text: string): Promise<void> {
    await this.page.getByText(text).first().click()
  }

  async hasText(text: string): Promise<boolean> {
    return (await this.page.getByText(text).count()) > 0
  }

  async waitText(text: string, timeoutMs = TIMEOUT_MS): Promise<void> {
    await this.page.getByText(text).first().waitFor({ state: 'visible', timeout: timeoutMs })
  }

  async waitNoText(text: string, timeoutMs = TIMEOUT_MS): Promise<void> {
    await this.page.getByText(text).first().waitFor({ state: 'hidden', timeout: timeoutMs })
  }

  resize(width: number, height: number): Promise<void> {
    return this.page.setViewportSize({ width, height })
  }

  pause(ms: number): Promise<void> {
    return this.page.waitForTimeout(ms)
  }

  async shot(name: string, options: ShotOptions = {}): Promise<void> {
    await this.page.waitForTimeout(SETTLE_MS)
    const png = await this.page.screenshot({ type: 'png' })
    // The page title is "<session name> · ref" (web/src/App.vue).
    const sessionName = (await this.page.title()).replace(/ · ref$/, '')
    await publishImage(png, shotFileName(this.kind, name, this.lang), { readme: options.readme ?? false, readmeName: options.readmeName, sessionName })
  }

  // The page for scenarios that need Playwright itself (attaching a file).
  get playwrightPage(): Page {
    return this.page
  }
}

export type { WebUi }

// Runs `body` against a fresh `ref-web --mock` with its own state directories.
export async function withWebUi(browser: Browser, lang: Lang, body: (ui: WebUi) => Promise<void>): Promise<void> {
  const isolated = isolatedEnv()
  const web = await startMockWeb(isolated)
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: DEVICE_SCALE_FACTOR,
    locale: BROWSER_LOCALE[lang],
    colorScheme: 'dark',
    permissions: ['notifications'], // keeps the "allow notifications?" bar out of the pictures
  })
  try {
    const page = await context.newPage()
    page.setDefaultTimeout(TIMEOUT_MS)
    await page.goto(web.loginUrl)
    await page.waitForSelector('textarea:not([disabled])')
    try {
      await body(new WebUi(page, lang))
    } catch (error) {
      await page.screenshot({ path: FAILURE_SHOT }) // what the page looked like when it went wrong
      throw error
    }
  } finally {
    await context.close()
    await web.stop()
    isolated.remove()
  }
}
