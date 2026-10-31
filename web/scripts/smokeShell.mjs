// The window around the conversation: sidebar, sessions, command palette, dialogs, split view.
import { join } from 'node:path'
import { OUT, WAIT_MS } from './smokeLib.mjs'

const SETTLE_MS = 500
const LOGIN_LOST_WATCH_MS = 12000 // longer than the longest retry delay (8 s)
const ROWS = '.sidebar .running .row'
const DIALOG = '[role="dialog"]'
const VIEWPORT_WIDTH = 980
const SPLIT_VIEWPORT = { width: 1500, height: 900 }
const SPLIT_TILES = 4
const ARROW_KEYS = ['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp']
const SIZING_HEIGHTS = [900, 600]
const SIZING_COUNTS = [4, 8]
const HISTORY_ROW_PX = 44 // Theme.sidebarHistoryRowHeight
const MIN_HISTORY_ROWS = 3 // Theme.sidebarMinHistoryRows
const MIN_RUNNING_ROWS = 3 // Theme.sidebarMinRunningRows

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

// ref-web forgets the browser's login when it restarts: the page says so and stops reconnecting.
export async function loginLostScenario(page, problems, restartServer) {
  const before = problems.length
  await restartServer()
  const banner = page.locator('[role="alert"]', { hasText: 'Not logged in' })
  await banner.waitFor({ timeout: WAIT_MS })
  await shot(page, 'not-logged-in')
  // The refused reconnects are expected; anything else stays a problem.
  const expected = /401|WebSocket|ERR_CONNECTION_REFUSED/
  const kept = problems.slice(before).filter((problem) => !expected.test(problem))
  problems.splice(before, problems.length - before, ...kept)

  let attempts = 0
  page.on('request', (request) => {
    if (request.url().includes('/ws')) attempts++
  })
  await page.waitForTimeout(LOGIN_LOST_WATCH_MS)
  if (attempts > 0) problems.push(`the page kept reconnecting after the login was lost (${attempts} tries)`)
}

async function sidebarGeometry(page) {
  return page.evaluate(() => {
    const box = (selector) => document.querySelector(selector).getBoundingClientRect()
    const running = document.querySelector('.sidebar .running')
    return {
      running: box('.sidebar .running').height,
      runningScrolls: running.scrollHeight > running.clientHeight,
      runningRows: running.querySelectorAll('.row').length,
      history: box('.sidebar .history').height,
    }
  })
}

// Running keeps its rows until History would get fewer than three rows; below that it scrolls.
export async function sidebarSizingScenario(page, problems) {
  for (const wanted of SIZING_COUNTS) {
    while ((await count(page, ROWS)) < wanted) {
      await page.keyboard.press('Alt+KeyN')
      await page.waitForTimeout(SETTLE_MS / 5)
    }
    for (const height of SIZING_HEIGHTS) {
      await page.setViewportSize({ width: VIEWPORT_WIDTH, height })
      await shot(page, `sidebar-${wanted}-rows-${height}px`)
      const geometry = await sidebarGeometry(page)
      console.log(`sidebar ${wanted} running, ${height}px:`, JSON.stringify(geometry))
      const minHistory = MIN_HISTORY_ROWS * HISTORY_ROW_PX
      const runningAtMinimum = geometry.runningRows >= MIN_RUNNING_ROWS && geometry.runningScrolls
      if (geometry.history < minHistory && !runningAtMinimum)
        problems.push(`sidebar ${wanted} running at ${height}px: history ${geometry.history}px is too small`)
      if (geometry.history > minHistory && geometry.runningScrolls)
        problems.push(`sidebar ${wanted} running at ${height}px: running scrolls while history has room`)
    }
  }
  await page.setViewportSize({ width: VIEWPORT_WIDTH, height: SIZING_HEIGHTS[0] })
}

// What the four tiles show of their composers: which are visible, whether each keeps its status line,
// which tile is focused and whether the caret is in its box.
async function tileComposers(page) {
  return page.evaluate(() =>
    [...document.querySelectorAll('[data-tile]')].map((tile) => {
      const slot = tile.querySelector('.box-slot')
      const box = tile.querySelector('textarea')
      const style = getComputedStyle(slot)
      return {
        composerVisible: slot.getBoundingClientRect().height > 0 && style.visibility === 'visible',
        statusLine: tile.querySelector('.statusline')?.getBoundingClientRect().height > 0,
        focused: tile.classList.contains('focused'),
        caretInBox: document.activeElement === box,
        draft: box.value,
      }
    }),
  )
}

async function expectOnlyFocusedComposer(page, problems, what) {
  await page.waitForTimeout(SETTLE_MS)
  const tiles = await tileComposers(page)
  const focused = tiles.findIndex((tile) => tile.focused)
  const visible = tiles.flatMap((tile, index) => (tile.composerVisible ? [index] : []))
  if (focused < 0 || visible.length !== 1 || visible[0] !== focused)
    problems.push(`${what}: composers visible in tiles [${visible}], focus in tile ${focused}`)
  if (focused >= 0 && !tiles[focused].caretInBox)
    problems.push(`${what}: the caret is not in the focused tile's box`)
  if (tiles.some((tile) => !tile.statusLine)) problems.push(`${what}: a tile lost its status line`)
  return { tiles, focused }
}

// In split view only the focused tile has an input box; the drafts of the others stay.
export async function splitComposerScenario(page, problems) {
  await page.setViewportSize(SPLIT_VIEWPORT)
  while ((await count(page, ROWS)) < SPLIT_TILES) {
    await page.keyboard.press('Alt+KeyN')
    await page.waitForTimeout(SETTLE_MS / 5)
  }
  await page.keyboard.press('Alt+KeyW')
  await expectCount(page, problems, '[data-tile]', SPLIT_TILES, 'tiles for the composer check')
  const first = await expectOnlyFocusedComposer(page, problems, 'split view start')

  const draft = 'a half-written prompt'
  await page.locator('[data-tile] textarea').nth(first.focused).fill(draft)

  // A click on another tile's header moves the box there.
  const other = (first.focused + 1) % SPLIT_TILES
  await page.locator('[data-tile]').nth(other).locator('.header .title').click()
  const afterClick = await expectOnlyFocusedComposer(page, problems, 'after a click')
  if (afterClick.focused !== other) problems.push(`click focused tile ${afterClick.focused}, wanted ${other}`)
  if (afterClick.tiles[first.focused].draft !== draft)
    problems.push('the draft of the unfocused tile was lost')
  await shot(page, 'split-click-focus')

  // Alt+arrows move it by keyboard, and the old draft comes back with its tile.
  let focused = afterClick.focused
  let tiles = afterClick.tiles
  for (const key of ARROW_KEYS) {
    if (tiles[focused].draft === draft) break
    await page.keyboard.press(`Alt+${key}`)
    const step = await expectOnlyFocusedComposer(page, problems, `after Alt+${key}`)
    focused = step.focused
    tiles = step.tiles
    await shot(page, `split-keyboard-focus-${key}`)
  }
  const draftHolder = tiles.findIndex((tile) => tile.draft === draft)
  if (draftHolder < 0) problems.push('the draft vanished after the focus round trip')
  else if (draftHolder !== focused) problems.push('Alt+arrows did not reach the tile with the draft')
  else await page.locator('[data-tile] textarea').nth(draftHolder).fill('')

  await page.keyboard.press('Alt+KeyW')
  await expectCount(page, problems, '[data-tile]', 0, 'tiles after the composer check')
  await page.setViewportSize({ width: VIEWPORT_WIDTH, height: SIZING_HEIGHTS[0] })
}
