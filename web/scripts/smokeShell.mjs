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
const MIN_FOCUS_TILES = 2
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
  await page.locator('[data-tile]').nth(other).locator(':scope > header .title').click()
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

const PRESS_HOLD_MS = 120 // a real click holds the button for a moment between down and up
const FRAME_SAMPLE_MS = 1200
const END_TOLERANCE_PX = 1.5
const PERMISSION_GUARD_WAIT_MS = 500 // longer than the 350 ms during which keys do not answer

// Samples how far tile `index`'s conversation is from its end each time the conversation changes
// size, after the page's own observers have run (what the next paint shows), and once at the end.
async function sampleGapToEnd(page, index, durationMs) {
  return page.evaluate(
    ([tileIndex, duration]) =>
      new Promise((resolve) => {
        const scroller = document.querySelectorAll('[data-tile]')[tileIndex].querySelector('.scroller')
        const gap = () => scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop
        const gaps = [gap()]
        const observer = new ResizeObserver(() => gaps.push(gap()))
        observer.observe(scroller)
        setTimeout(() => {
          observer.disconnect()
          gaps.push(gap())
          resolve(gaps)
        }, duration)
      }),
    [index, durationMs],
  )
}

async function focusedTile(page) {
  return page.evaluate(() =>
    [...document.querySelectorAll('[data-tile]')].findIndex((t) => t.classList.contains('focused')),
  )
}

// What the sidebar marks as the current session, as the index of the tile showing it.
async function highlightedTile(page) {
  return page.evaluate(() => {
    const names = [...document.querySelectorAll('[data-tile] > header .title')].map((e) => e.textContent)
    const marked = document.querySelector('.sidebar .running .row.current .title')?.textContent
    return names.indexOf(marked ?? '')
  })
}

async function startSplit(page, problems, tiles) {
  await page.setViewportSize(SPLIT_VIEWPORT)
  while ((await count(page, ROWS)) < tiles) {
    await page.keyboard.press('Alt+KeyN')
    await page.waitForTimeout(SETTLE_MS / 5)
  }
  const running = await count(page, ROWS)
  await page.keyboard.press('Alt+KeyW')
  await expectCount(page, problems, '[data-tile]', running, 'tiles for the split checks')
  await page.waitForTimeout(SETTLE_MS)
}

async function leaveSplit(page, problems) {
  await page.keyboard.press('Alt+KeyW')
  await expectCount(page, problems, '[data-tile]', 0, 'tiles after the split checks')
  await page.setViewportSize({ width: VIEWPORT_WIDTH, height: SIZING_HEIGHTS[0] })
}

// Starts the permission script in tile 0 and moves the focus to tile 1, so that tile 0 asks while unfocused.
async function askInBackground(page) {
  await page.locator('[data-tile]').nth(0).locator(':scope > header .title').click()
  await page.locator('[data-tile] textarea').nth(0).fill('permission')
  await page.keyboard.press('Enter')
  await page.locator('[data-tile]').nth(1).locator(':scope > header .title').click()
  const card = page.locator('[data-tile]').nth(0).locator('.permission.open')
  await card.waitFor({ timeout: WAIT_MS })
  await page.waitForTimeout(SETTLE_MS)
  return card
}

const WHEEL_STEP_PX = 120
const WHEEL_MAX_STEPS = 30

// Scrolls tile `index` with the mouse wheel until `target` is inside its conversation, as a user
// would. Returns the target's box, or undefined when it never comes into view.
async function scrollIntoViewByWheel(page, index, target) {
  const scroller = page.locator('[data-tile]').nth(index).locator('.scroller')
  const view = await scroller.boundingBox()
  if (!view) return undefined
  await page.mouse.move(view.x + view.width / 2, view.y + view.height / 2)
  for (let step = 0; step < WHEEL_MAX_STEPS; step++) {
    const box = await target.boundingBox()
    if (!box) return undefined
    if (box.y >= view.y && box.y + box.height <= view.y + view.height) return box
    await page.mouse.wheel(0, box.y < view.y ? -WHEEL_STEP_PX : WHEEL_STEP_PX)
    await page.waitForTimeout(SETTLE_MS / 10)
  }
  return undefined
}

// Answers the requests of tile `index` with the `a` key, once the guard time has passed, until its turn ends.
async function finishTurnByKey(page, problems, index) {
  const tile = page.locator('[data-tile]').nth(index)
  const deadline = Date.now() + WAIT_MS * 2
  while (!(await tile.locator('button.send:not(.busy)').isVisible())) {
    if (Date.now() > deadline) {
      problems.push('the permission key did not get the turn through')
      return
    }
    if (await tile.locator('.permission.open').isVisible()) {
      await page.waitForTimeout(PERMISSION_GUARD_WAIT_MS)
      await page.keyboard.press('a')
    }
    await page.waitForTimeout(SETTLE_MS / 5)
  }
}

// Pointer and keyboard in the split view: a press on a button of an unfocused tile reaches the
// button, the sidebar follows the focus, and the permission keys work as in the single view.
export async function splitFocusScenario(page, problems) {
  await startSplit(page, problems, MIN_FOCUS_TILES)

  // 1a. Focusing a tile unfolds its box below a conversation that ends in a request card: the end stays in view.
  const card = await askInBackground(page)
  const gapBefore = (await sampleGapToEnd(page, 0, 50)).at(-1)
  await page.locator('[data-tile]').nth(0).locator(':scope > header .title').click()
  const gaps = await sampleGapToEnd(page, 0, FRAME_SAMPLE_MS)
  console.log('split unfold: gap before', gapBefore, 'max during', Math.max(...gaps))
  if (Math.max(...gaps) > gapBefore + END_TOLERANCE_PX)
    problems.push(`the conversation moved ${Math.max(...gaps)}px off its end while the box unfolded`)
  await shot(page, 'split-unfolded-over-card')

  // 1b. Allow in an unfocused tile: the press reaches the button and the tile takes the focus.
  await page.locator('[data-tile]').nth(1).locator(':scope > header .title').click()
  await page.waitForTimeout(SETTLE_MS)
  const tools = page.locator('[data-tile]').nth(0).locator('.permission')
  const toolsBefore = await tools.count()
  const allow = card.locator('button.choice.success')
  const box = await scrollIntoViewByWheel(page, 0, allow)
  if (!box) problems.push('no Allow button in the unfocused tile')
  else {
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await page.mouse.down()
    await page.waitForTimeout(PRESS_HOLD_MS)
    await page.mouse.up()
    await page.waitForTimeout(SETTLE_MS)
    if ((await focusedTile(page)) !== 0) problems.push('the click on Allow did not focus the tile')
    // The script asks again for its next tool: the first request is answered when a next block appears or none asks.
    const answered = (await tools.count()) > toolsBefore || !(await card.first().isVisible())
    if (!answered) problems.push('the click on Allow in an unfocused tile did not answer the request')
    await shot(page, 'split-click-allow')
  }
  await finishTurnByKey(page, problems, 0)

  // 2. Alt+arrow moves the focus and the sidebar highlight with it.
  for (const key of ['ArrowRight', 'ArrowLeft']) {
    await page.keyboard.press(`Alt+${key}`)
    await page.waitForTimeout(SETTLE_MS / 3)
    const focused = await focusedTile(page)
    const marked = await highlightedTile(page)
    if (focused !== marked)
      problems.push(`after Alt+${key} tile ${focused} has the focus, the sidebar marks ${marked}`)
  }
  await page.locator('[data-tile]').nth(1).locator(':scope > header .title').click()
  await page.waitForTimeout(SETTLE_MS / 3)
  if ((await highlightedTile(page)) !== 1) problems.push('the sidebar does not follow a click on a tile')
  await shot(page, 'split-sidebar-follows')

  // 3. The permission key in the focused tile: after the guard time it answers and is not typed.
  await page.locator('[data-tile]').nth(0).locator(':scope > header .title').click()
  await page.locator('[data-tile] textarea').nth(0).fill('permission')
  await page.keyboard.press('Enter')
  await page.locator('[data-tile]').nth(0).locator('.permission.open').waitFor({ timeout: WAIT_MS })
  await page.waitForTimeout(PERMISSION_GUARD_WAIT_MS)
  await page.keyboard.press('a')
  await page.waitForTimeout(SETTLE_MS / 3)
  const typed = await page.locator('[data-tile] textarea').nth(0).inputValue()
  if (typed !== '') problems.push(`the permission key was typed into the box: "${typed}"`)
  await finishTurnByKey(page, problems, 0)
  const left = await page.locator('[data-tile] textarea').nth(0).inputValue()
  if (left !== '') problems.push(`the permission key was typed into the box: "${left}"`)

  await leaveSplit(page, problems)
}
