// The window around the conversation: sidebar, sessions, command palette, dialogs, split view.
import { join } from 'node:path'
import { OUT, WAIT_MS } from './smokeLib.mjs'

const SETTLE_MS = 500
const ROWS = '.sidebar .running .row'
const DIALOG = '[role="dialog"]'

async function count(page, selector) {
  return page.locator(selector).count()
}

async function expectCount(page, problems, selector, expected, what) {
  try {
    await page.waitForFunction(
      ([sel, want]) => document.querySelectorAll(sel).length === want,
      [selector, expected],
      { timeout: WAIT_MS },
    )
  } catch {
    problems.push(`${what}: expected ${expected}, found ${await count(page, selector)}`)
  }
}

async function shot(page, name) {
  await page.waitForTimeout(SETTLE_MS)
  await page.screenshot({ path: join(OUT, `shell-${name}.png`) })
}

async function closeDialog(page, problems, what) {
  await page.keyboard.press('Escape')
  await expectCount(page, problems, DIALOG, 0, `${what} did not close with Esc`)
}

export async function shellScenario(page, problems) {
  await expectCount(page, problems, ROWS, 1, 'running sessions at start')

  // Alt+N starts a session in the same directory; the sidebar lists both.
  await page.keyboard.press('Alt+KeyN')
  await expectCount(page, problems, ROWS, 2, 'Alt+N did not add a session')
  await shot(page, 'two-sessions')

  // Clicking a row switches the session.
  const opened = page.locator(ROWS).nth(0)
  await opened.click()
  await expectCount(page, problems, `${ROWS}.current`, 1, 'current row after click')

  // Command palette: search, run the Sessions command, Esc closes.
  await page.keyboard.press('Control+KeyP')
  await expectCount(page, problems, '.palette', 1, 'Ctrl+P did not open the palette')
  await shot(page, 'palette')
  await page.keyboard.type('sessions')
  await page.keyboard.press('Enter')
  await page.waitForSelector(`${DIALOG}[aria-label="Sessions"]`, { timeout: WAIT_MS })
  await shot(page, 'sessions')
  await closeDialog(page, problems, 'sessions dialog')

  // Models dialog from its shortcut, with the current model checked.
  await page.keyboard.press('Alt+KeyM')
  await page.waitForSelector(`${DIALOG}[aria-label="Switch model"]`, { timeout: WAIT_MS })
  await shot(page, 'models')
  await closeDialog(page, problems, 'models dialog')

  // Help is generated from the registry.
  await page.keyboard.press('Alt+KeyH')
  await page.waitForSelector(`${DIALOG}[aria-label="Help"]`, { timeout: WAIT_MS })
  await shot(page, 'help')
  await closeDialog(page, problems, 'help dialog')

  // Settings: switching the theme from the segmented control.
  await page.keyboard.press('Alt+KeyS')
  await page.waitForSelector(`${DIALOG}[aria-label="Settings"]`, { timeout: WAIT_MS })
  await page.click(`${DIALOG} [role="radio"]:has-text("Dark")`)
  await shot(page, 'settings-dark')
  if ((await page.evaluate(() => document.documentElement.dataset.theme)) !== 'dark')
    problems.push('the Dark segment did not switch the theme')
  await page.click(`${DIALOG} [role="radio"]:has-text("Light")`)
  await closeDialog(page, problems, 'settings dialog')

  // Split view shows a tile per session; Alt+arrows move the focus; zoom and back.
  await page.keyboard.press('Alt+KeyW')
  await expectCount(page, problems, '[data-tile]', 2, 'tiles in split view')
  await shot(page, 'split')
  await page.keyboard.press('Alt+KeyZ')
  await shot(page, 'zoom')
  await page.keyboard.press('Alt+KeyZ')
  await page.keyboard.press('Alt+KeyW')
  await expectCount(page, problems, '[data-tile]', 0, 'tiles after leaving split view')

  // Sidebar toggle and closing a session.
  await page.keyboard.press('Alt+KeyB')
  await shot(page, 'sidebar-hidden')
  await page.keyboard.press('Alt+KeyB')
  await page.keyboard.press('Alt+KeyK')
  await expectCount(page, problems, ROWS, 1, 'Alt+K did not close the session')
  await shot(page, 'closed')
}
