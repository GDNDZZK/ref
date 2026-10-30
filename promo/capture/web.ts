// Screenshots of the WebUI running against `ref-web --mock` in headless Chromium.
import type { Browser, Page } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import { isolatedEnv, startMockWeb } from './lib/backend.ts'
import { publishImage } from './lib/publish.ts'

const VIEWPORT = { width: 1600, height: 1000 }
const DEVICE_SCALE_FACTOR = 2 // crisp when a video zooms in
const PAGE_TIMEOUT_MS = 20_000

type Theme = 'dark' | 'light'

interface WebShot {
  name: string
  theme: Theme
  locale: string
  readme: boolean
  // Drives the page to the state to photograph.
  prepare(page: Page): Promise<void>
}

const NO_SESSION_NOTICE = 'no session is open'
const EMPTY_STATE_HEADING = 'What are we building?'
const CONNECT_SETTLE_MS = 600

// The notice shows briefly while the page connects; the empty-state heading is there the
// whole time. Ready = heading present and notice absent, still so after a short wait.
async function waitForSession(page: Page): Promise<void> {
  const ready = () =>
    page.waitForFunction(
      ([heading, notice]) => {
        const text = document.body.innerText
        return text.includes(heading!) && !text.includes(notice!)
      },
      [EMPTY_STATE_HEADING, NO_SESSION_NOTICE],
    )
  await ready()
  await page.waitForTimeout(CONNECT_SETTLE_MS)
  await ready()
}

// Sends `prompt` once the page has a session, and waits until `replyText` (a phrase the mock
// script prints) is on the page and the turn has ended.
async function runMockScript(page: Page, prompt: string, replyText: string): Promise<void> {
  await waitForSession(page)
  await page.fill('textarea', prompt)
  await page.keyboard.press('Enter')
  await page.waitForSelector(`text=${replyText}`)
  await waitForIdle(page)
}

// The send button turns into a stop button while a turn runs.
async function waitForIdle(page: Page): Promise<void> {
  await page.waitForSelector('button[aria-label="Send"]', { state: 'attached', timeout: PAGE_TIMEOUT_MS })
}

const SHOTS: WebShot[] = [
  {
    name: 'web-chat-dark',
    theme: 'dark',
    locale: 'en-US',
    readme: true,
    async prepare(page) {
      await runMockScript(page, 'markdown', 'A block without a language')
    },
  },
  {
    name: 'web-chat-light',
    theme: 'light',
    locale: 'en-US',
    readme: false,
    async prepare(page) {
      await runMockScript(page, 'markdown', 'A block without a language')
    },
  },
]

async function captureShot(browser: Browser, loginUrl: string, shot: WebShot): Promise<void> {
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: DEVICE_SCALE_FACTOR,
    locale: shot.locale,
    colorScheme: shot.theme,
  })
  try {
    const page = await context.newPage()
    page.setDefaultTimeout(PAGE_TIMEOUT_MS)
    await page.goto(loginUrl)
    await page.waitForSelector('textarea')
    await shot.prepare(page)
    await publishImage(await page.screenshot({ type: 'png' }), shot.name, { readme: shot.readme })
  } finally {
    await context.close()
  }
}

export async function captureWeb(): Promise<void> {
  const isolated = isolatedEnv()
  const web = await startMockWeb(isolated)
  const browser = await launchChromium()
  try {
    for (const shot of SHOTS) await captureShot(browser, web.loginUrl, shot)
  } finally {
    await browser.close()
    await web.stop()
    isolated.remove()
  }
}
