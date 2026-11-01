// The window around the conversation: sidebar, sessions, command palette, dialogs, split view.
import { join } from 'node:path'
import { errors } from 'playwright-core'
import { INPUT, OUT, SEND_IDLE, WAIT_MS, sendPrompt } from './smokeLib.mjs'

const SETTLE_MS = 500
const SHORT_SETTLE_MS = SETTLE_MS / 3
const POLL_MS = SETTLE_MS / 5
const WHEEL_SETTLE_MS = SETTLE_MS / 10
const GAP_PROBE_MS = 50
const TILE = '[data-tile]'
const SHORTCUT = {
  newSession: 'Alt+KeyN',
  closeSession: 'Alt+KeyK',
  toggleSplit: 'Alt+KeyW',
  zoom: 'Alt+KeyZ',
  toggleSidebar: 'Alt+KeyB',
  models: 'Alt+KeyM',
  help: 'Alt+KeyH',
  settings: 'Alt+KeyS',
}
const LOGIN_LOST_WATCH_MS = 12000 // longer than the longest retry delay (8 s)
const ROWS = '.sidebar .running .row'
const DIALOG = '[role="dialog"]'
const VIEWPORT_WIDTH = 980
const SPLIT_VIEWPORT = { width: 1500, height: 900 }
const SPLIT_TILES = 4
const LOCKUP_SIZE = { width: 60, height: 24 }
const MIN_FOCUS_TILES = 5
const CHROME_VIEWPORT = { width: 1440, height: 900 }
const PROMPT_CLICK_TIMEOUT_MS = 1000
const META_ROW_PX = 24 // one row under a message
const ARROW_KEYS = ['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp']
const SIZING_HEIGHTS = [900, 600]
const SIZING_COUNTS = [4, 8]
const HISTORY_ROW_PX = 44 // Theme.sidebarHistoryRowHeight
const MIN_HISTORY_ROWS = 3 // Theme.sidebarMinHistoryRows
const MIN_RUNNING_ROWS = 3 // Theme.sidebarMinRunningRows

async function count(page, selector) {
  return page.locator(selector).count()
}

async function clickTileHeader(page, index) {
  await page.locator(TILE).nth(index).locator(':scope > header .title').click()
}

function tileInput(page, index) {
  return page.locator(`${TILE} ${INPUT}`).nth(index)
}

async function ensureSessionCount(page, wanted) {
  while ((await count(page, ROWS)) < wanted) {
    await page.keyboard.press(SHORTCUT.newSession)
    await page.waitForTimeout(POLL_MS)
  }
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
  await page.keyboard.press(SHORTCUT.newSession)
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
  await page.keyboard.press(SHORTCUT.models)
  await page.waitForSelector(`${DIALOG}[aria-label="Switch model"]`, { timeout: WAIT_MS })
  await shot(page, 'models')
  await closeDialog(page, problems, 'models dialog')

  // Help is generated from the registry.
  await page.keyboard.press(SHORTCUT.help)
  await page.waitForSelector(`${DIALOG}[aria-label="Help"]`, { timeout: WAIT_MS })
  await shot(page, 'help')
  await closeDialog(page, problems, 'help dialog')

  // Settings: switching the theme from the segmented control.
  await page.keyboard.press(SHORTCUT.settings)
  await page.waitForSelector(`${DIALOG}[aria-label="Settings"]`, { timeout: WAIT_MS })
  await page.click(`${DIALOG} [role="radio"]:has-text("Dark")`)
  await shot(page, 'settings-dark')
  if ((await page.evaluate(() => document.documentElement.dataset.theme)) !== 'dark')
    problems.push('the Dark segment did not switch the theme')
  await page.click(`${DIALOG} [role="radio"]:has-text("Light")`)
  await closeDialog(page, problems, 'settings dialog')

  // Split view shows a tile per session; Alt+arrows move the focus; zoom and back.
  await page.keyboard.press(SHORTCUT.toggleSplit)
  await expectCount(page, problems, TILE, 2, 'tiles in split view')
  await shot(page, 'split')
  await page.keyboard.press(SHORTCUT.zoom)
  await shot(page, 'zoom')
  await page.keyboard.press(SHORTCUT.zoom)
  await page.keyboard.press(SHORTCUT.toggleSplit)
  await expectCount(page, problems, TILE, 0, 'tiles after leaving split view')

  // Sidebar toggle and closing a session.
  await page.keyboard.press(SHORTCUT.toggleSidebar)
  await shot(page, 'sidebar-hidden')
  await page.keyboard.press(SHORTCUT.toggleSidebar)
  await page.keyboard.press(SHORTCUT.closeSession)
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
    await ensureSessionCount(page, wanted)
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
  await ensureSessionCount(page, SPLIT_TILES)
  await page.keyboard.press(SHORTCUT.toggleSplit)
  await expectCount(page, problems, TILE, SPLIT_TILES, 'tiles for the composer check')
  const first = await expectOnlyFocusedComposer(page, problems, 'split view start')

  const draft = 'a half-written prompt'
  await tileInput(page, first.focused).fill(draft)

  // A click on another tile's header moves the box there.
  const other = (first.focused + 1) % SPLIT_TILES
  await clickTileHeader(page, other)
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
  else await tileInput(page, draftHolder).fill('')

  await page.keyboard.press(SHORTCUT.toggleSplit)
  await expectCount(page, problems, TILE, 0, 'tiles after the composer check')
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
  await ensureSessionCount(page, tiles)
  const running = await count(page, ROWS)
  await page.keyboard.press(SHORTCUT.toggleSplit)
  await expectCount(page, problems, TILE, running, 'tiles for the split checks')
  await page.waitForTimeout(SETTLE_MS)
}

async function leaveSplit(page, problems) {
  await page.keyboard.press(SHORTCUT.toggleSplit)
  await expectCount(page, problems, TILE, 0, 'tiles after the split checks')
  await page.setViewportSize({ width: VIEWPORT_WIDTH, height: SIZING_HEIGHTS[0] })
}

// Starts the permission script in tile 0 and moves the focus to tile 1, so that tile 0 asks while unfocused.
async function askInBackground(page) {
  await clickTileHeader(page, 0)
  await sendPrompt(page, 'permission', tileInput(page, 0))
  await clickTileHeader(page, 1)
  const card = page.locator(TILE).nth(0).locator('.permission.open')
  await card.waitFor({ timeout: WAIT_MS })
  await page.waitForTimeout(SETTLE_MS)
  return card
}

const WHEEL_STEP_PX = 120
const WHEEL_MAX_STEPS = 30

// Scrolls tile `index` with the mouse wheel until `target` is inside its conversation, as a user
// would. Returns the target's box, or undefined when it never comes into view.
async function scrollIntoViewByWheel(page, index, target) {
  const scroller = page.locator(TILE).nth(index).locator('.scroller')
  const view = await scroller.boundingBox()
  if (!view) return undefined
  await page.mouse.move(view.x + view.width / 2, view.y + view.height / 2)
  for (let step = 0; step < WHEEL_MAX_STEPS; step++) {
    const box = await target.boundingBox()
    if (!box) return undefined
    if (box.y >= view.y && box.y + box.height <= view.y + view.height) return box
    await page.mouse.wheel(0, box.y < view.y ? -WHEEL_STEP_PX : WHEEL_STEP_PX)
    await page.waitForTimeout(WHEEL_SETTLE_MS)
  }
  return undefined
}

// Answers the requests of tile `index` with the `a` key, once the guard time has passed, until its turn ends.
async function finishTurnByKey(page, problems, index) {
  const tile = page.locator(TILE).nth(index)
  const deadline = Date.now() + WAIT_MS * 2
  while (!(await tile.locator(SEND_IDLE).isVisible())) {
    if (Date.now() > deadline) {
      problems.push('the permission key did not get the turn through')
      return
    }
    if (await tile.locator('.permission.open').isVisible()) {
      await page.waitForTimeout(PERMISSION_GUARD_WAIT_MS)
      await page.keyboard.press('a')
    }
    await page.waitForTimeout(POLL_MS)
  }
}

// Focusing a tile unfolds its box below a conversation that ends in a request card: the end stays in view.
async function checkUnfoldOverCard(page, problems) {
  const gapBefore = (await sampleGapToEnd(page, 0, GAP_PROBE_MS)).at(-1)
  await clickTileHeader(page, 0)
  const gaps = await sampleGapToEnd(page, 0, FRAME_SAMPLE_MS)
  console.log('split unfold: gap before', gapBefore, 'max during', Math.max(...gaps))
  if (Math.max(...gaps) > gapBefore + END_TOLERANCE_PX)
    problems.push(`the conversation moved ${Math.max(...gaps)}px off its end while the box unfolded`)
  await shot(page, 'split-unfolded-over-card')
}

// Allow in an unfocused tile: the press reaches the button and the tile takes the focus.
async function checkAllowClickInUnfocusedTile(page, problems, card) {
  await clickTileHeader(page, 1)
  await page.waitForTimeout(SETTLE_MS)
  const tools = page.locator(TILE).nth(0).locator('.permission')
  const toolsBefore = await tools.count()
  const allow = card.locator('button.choice.success')
  // The buttons are at the end of the card, in view without any scrolling.
  const scrollerBox = await page.locator(TILE).nth(0).locator('.scroller').boundingBox()
  const allowBox = await allow.boundingBox()
  if (
    !allowBox ||
    !scrollerBox ||
    allowBox.y < scrollerBox.y ||
    allowBox.y + allowBox.height > scrollerBox.y + scrollerBox.height
  )
    problems.push('the Allow buttons of a waiting request are not in view')
  const box = await scrollIntoViewByWheel(page, 0, allow)
  if (!box) {
    problems.push('no Allow button in the unfocused tile')
    return
  }
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

// Alt+arrow moves the focus and the sidebar highlight with it.
async function checkSidebarFollowsFocus(page, problems) {
  for (const key of ['ArrowRight', 'ArrowLeft']) {
    await page.keyboard.press(`Alt+${key}`)
    await page.waitForTimeout(SHORT_SETTLE_MS)
    const focused = await focusedTile(page)
    const marked = await highlightedTile(page)
    if (focused !== marked)
      problems.push(`after Alt+${key} tile ${focused} has the focus, the sidebar marks ${marked}`)
  }
  await clickTileHeader(page, 1)
  await page.waitForTimeout(SHORT_SETTLE_MS)
  if ((await highlightedTile(page)) !== 1) problems.push('the sidebar does not follow a click on a tile')
  await shot(page, 'split-sidebar-follows')
}

// The permission key in the focused tile: after the guard time it answers and is not typed.
async function checkPermissionKeyInFocusedTile(page, problems) {
  await clickTileHeader(page, 0)
  await sendPrompt(page, 'permission', tileInput(page, 0))
  await page.locator(TILE).nth(0).locator('.permission.open').waitFor({ timeout: WAIT_MS })
  await page.waitForTimeout(PERMISSION_GUARD_WAIT_MS)
  await page.keyboard.press('a')
  await page.waitForTimeout(SHORT_SETTLE_MS)
  const typed = await tileInput(page, 0).inputValue()
  if (typed !== '') problems.push(`the permission key was typed into the box: "${typed}"`)
  await finishTurnByKey(page, problems, 0)
  const left = await tileInput(page, 0).inputValue()
  if (left !== '') problems.push(`the permission key was typed into the box: "${left}"`)
}

// Three columns at 1440 px with the sidebar hidden: the floating window buttons stay clear of the
// tile headers, and the line under a reply keeps to one row.
async function checkTileChrome(page, problems) {
  await page.setViewportSize(CHROME_VIEWPORT)
  // The notification prompt pushes the tiles below the buttons; without it they sit at the top.
  try {
    await page.locator('button:has-text("Not now")').click({ timeout: PROMPT_CLICK_TIMEOUT_MS })
  } catch (error) {
    if (!(error instanceof errors.TimeoutError)) throw error // no prompt: nothing to dismiss
  }
  await page.keyboard.press(SHORTCUT.toggleSidebar)
  await page.waitForTimeout(SETTLE_MS)
  const report = await page.evaluate((metaRowPx) => {
    const rect = (element) => element.getBoundingClientRect()
    const hits = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
    const buttons = [...document.querySelectorAll('.corner button')].map(rect)
    const headerParts = '[data-tile] > header .title, [data-tile] > header .mark, [data-tile] > header .zoom'
    return {
      covered: [...document.querySelectorAll(headerParts)].filter((part) =>
        buttons.some((button) => hits(rect(part), button)),
      ).length,
      wrapped: [...document.querySelectorAll('[data-tile] .meta')].filter(
        (row) => rect(row).height > metaRowPx,
      ).length,
      columns: new Set(
        [...document.querySelectorAll('[data-tile]')].map((tile) => Math.round(rect(tile).left)),
      ).size,
    }
  }, META_ROW_PX)
  console.log('tile chrome:', JSON.stringify(report))
  if (report.covered > 0) problems.push(`${report.covered} tile header part(s) lie under the window buttons`)
  if (report.wrapped > 0) problems.push(`${report.wrapped} meta line(s) wrap onto a second row`)
  await shot(page, 'split-chrome')
  await page.keyboard.press(SHORTCUT.toggleSidebar)
}

// Pointer and keyboard in the split view: a press on a button of an unfocused tile reaches the
// button, the sidebar follows the focus, and the permission keys work as in the single view.
export async function splitFocusScenario(page, problems) {
  await startSplit(page, problems, MIN_FOCUS_TILES)
  const card = await askInBackground(page)
  await checkUnfoldOverCard(page, problems)
  await checkAllowClickInUnfocusedTile(page, problems, card)
  await finishTurnByKey(page, problems, 0)
  await checkSidebarFollowsFocus(page, problems)
  await checkPermissionKeyInFocusedTile(page, problems)
  await checkTileChrome(page, problems)
  await leaveSplit(page, problems)
}

// The sidebar's brand lockup keeps its pixel size and follows the theme.
async function checkLockup(page, problems) {
  const lockup = page.locator('.sidebar .lockup')
  const box = await lockup.boundingBox()
  if (box?.width !== LOCKUP_SIZE.width || box?.height !== LOCKUP_SIZE.height)
    problems.push(
      `the lockup is ${box?.width}x${box?.height}, not ${LOCKUP_SIZE.width}x${LOCKUP_SIZE.height}`,
    )
  const light = await lockup.getAttribute('src')
  await page.keyboard.press(SHORTCUT.settings)
  await page.click(`${DIALOG} [role="radio"]:has-text("Dark")`)
  await page.waitForTimeout(SETTLE_MS)
  const dark = await lockup.getAttribute('src')
  if (light === dark) problems.push('the lockup did not change with the theme')
  await page.click(`${DIALOG} [role="radio"]:has-text("Light")`)
  await closeDialog(page, problems, 'settings dialog')
}

// The Todo section shows the list of the session on screen and is gone for a session without one.
export async function sidebarTodoScenario(page, problems) {
  await checkLockup(page, problems)
  await sendPrompt(page, 'todo')
  const section = page.locator('.sidebar .todo-section')
  try {
    await section.waitFor({ timeout: WAIT_MS })
  } catch {
    problems.push('the Todo section did not appear for a session with a list')
    return
  }
  const heading = (await section.locator('.heading').innerText()).replace(/\s+/g, ' ')
  if (!/^Todo \d+\/\d+$/.test(heading)) problems.push(`the Todo heading reads "${heading}"`)
  await shot(page, 'sidebar-todo')
  await page.keyboard.press(SHORTCUT.newSession)
  await expectCount(page, problems, '.sidebar .todo-section', 0, 'Todo sections for a session without a list')
  await page.keyboard.press(SHORTCUT.closeSession)
}
