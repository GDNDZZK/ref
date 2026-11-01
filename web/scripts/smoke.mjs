// Smoke test in headless Chromium against `ref-web --mock` (a scripted model, no real LLM).
//   node scripts/smoke.mjs [scenario ...]      scenarios are mock script names (help lists them)
// Needs a built ref-web (cargo build -p ref-web) and system Chromium. Screenshots go to $SMOKE_OUT.
import { join } from 'node:path'
import { errors } from 'playwright-core'
import { INPUT, OUT, SEND_BUSY, SEND_IDLE, WAIT_MS, sendPrompt, withMockPage } from './smokeLib.mjs'
import {
  loginLostScenario,
  shellScenario,
  sidebarSizingScenario,
  splitComposerScenario,
  sidebarTodoScenario,
  splitFocusScenario,
} from './smokeShell.mjs'

const SCENARIOS =
  process.argv.length > 2 ? process.argv.slice(2) : ['tools', 'reasoning', 'markdown', 'permission', 'fail']
const END_TOLERANCE_PX = 1.5
const SETTLE_MS = 1500
const FAR_FROM_END_PX = 200
const SCROLLER_BLANK_X = 490 // a point inside the conversation
const SCROLLER_BLANK_Y = 300
const GUARD_WAIT_MS = 500 // longer than the 350 ms during which keys do not answer
const POLL_MS = 300
const INITIAL_PAINT_MS = 500
const SCROLL_SETTLE_MS = 600
const JUMP_SETTLE_MS = 2000
const THEME_SWITCH_MS = 800
const WHEEL_UP_PX = 2000
const MIN_HIGHLIGHT_COLORS = 3

// A reply that ends before the check looks is fine: only a timeout is ignored.
async function waitForBusyIfAny(page) {
  try {
    await page.waitForSelector(SEND_BUSY, { timeout: WAIT_MS })
  } catch (error) {
    if (!(error instanceof errors.TimeoutError)) throw error
  }
}

// Answers every permission request until the reply ends: the first by click, the rest by the
// "a" key, which works while the composer has the focus, once the guard time has passed.
async function answerPermissions(page, scenario) {
  const allow = page.locator('.permission.open button.choice.success')
  const idle = page.locator(SEND_IDLE)
  const deadline = Date.now() + WAIT_MS * 2
  let asked = 0
  while (!(await idle.isVisible())) {
    if (Date.now() > deadline) throw new Error(`scenario ${scenario} did not finish`)
    if (await allow.isVisible()) {
      const isFirstRequest = asked === 0
      asked++
      if (isFirstRequest) {
        await page.screenshot({ path: join(OUT, `${scenario}-permission.png`) })
        await allow.click()
      } else {
        await page.waitForTimeout(GUARD_WAIT_MS)
        await page.keyboard.press('a')
      }
    }
    await page.waitForTimeout(POLL_MS)
  }
}

async function gapToEnd(page) {
  return page.evaluate(() => {
    const scroller = document.querySelector('.scroller')
    return scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop
  })
}

async function stillEasing(page) {
  return page.evaluate(
    () => [...document.querySelectorAll('.frame')].filter((frame) => frame.style.height).length,
  )
}

// Scrolling up releases the end, the jump button brings it back, and a reply that streams in
// meanwhile does not pull the view down.
async function checkScrolling(page, problems) {
  await page.mouse.move(SCROLLER_BLANK_X, SCROLLER_BLANK_Y)
  await page.mouse.wheel(0, -WHEEL_UP_PX)
  await page.waitForTimeout(SCROLL_SETTLE_MS)
  if (!(await page.locator('.jump.shown').count())) problems.push('scrolling up did not show the jump button')
  await page.screenshot({ path: join(OUT, 'scrolled-up.png') })
  await page.click('.jump.shown button')
  await page.waitForTimeout(JUMP_SETTLE_MS)
  if ((await gapToEnd(page)) > END_TOLERANCE_PX)
    problems.push('the jump button did not bring the view to the end')

  await sendPrompt(page, 'slow')
  await page.waitForSelector(SEND_BUSY, { timeout: WAIT_MS })
  await page.mouse.wheel(0, -WHEEL_UP_PX)
  await page.waitForTimeout(SETTLE_MS)
  const gap = await gapToEnd(page)
  if (gap < FAR_FROM_END_PX)
    problems.push(`a streaming reply pulled the view back down (${gap}px from the end)`)
  await page.click(SEND_BUSY) // stop
  await page.waitForSelector(SEND_IDLE, { timeout: WAIT_MS })

  // The keyboard scrolls like the wheel: a streaming reply does not pull the view back.
  await page.click('.jump.shown button')
  await page.waitForTimeout(SETTLE_MS)
  await sendPrompt(page, 'slow')
  await page.waitForSelector(SEND_BUSY, { timeout: WAIT_MS })
  await page.mouse.click(SCROLLER_BLANK_X, SCROLLER_BLANK_Y)
  await page.keyboard.press('PageUp')
  await page.waitForTimeout(SETTLE_MS)
  const keyGap = await gapToEnd(page)
  if (keyGap < FAR_FROM_END_PX)
    problems.push(`a streaming reply pulled the view back down after PageUp (${keyGap}px from the end)`)
  await page.click(SEND_BUSY) // stop
  await page.waitForSelector(SEND_IDLE, { timeout: WAIT_MS })
}

// Code blocks get their grammar and colors (checked in the light theme).
async function checkHighlighting(page, problems) {
  const distinctColorCount = () =>
    page.evaluate(
      () =>
        new Set([...document.querySelectorAll('.code .token')].map((e) => getComputedStyle(e).color)).size,
    )
  const lightColorCount = await distinctColorCount()
  if (lightColorCount < MIN_HIGHLIGHT_COLORS)
    problems.push(`code blocks use only ${lightColorCount} color(s): not highlighted`)
}

// Every message has a footer, and Undo takes the last prompt back into the input box.
async function checkUndo(page, problems) {
  const footers = page.locator('.meta')
  if ((await footers.count()) === 0) problems.push('messages have no footer')
  const prompts = page.locator('.meta.user')
  const before = await prompts.count()
  await sendPrompt(page, 'tools')
  await waitForBusyIfAny(page)
  await page.waitForSelector(SEND_IDLE, { timeout: WAIT_MS })
  await prompts.last().hover()
  await prompts.last().locator('.action').click()
  await page
    .waitForFunction((count) => document.querySelectorAll('.meta.user').length === count, before, {
      timeout: WAIT_MS,
    })
    .catch(() => problems.push('undo did not remove the message'))
  const draft = await page.inputValue(INPUT)
  if (draft !== 'tools') problems.push(`undo put "${draft}" in the input box, not the prompt`)
  await page.fill(INPUT, '')
}

async function scenarios(page, problems) {
  await page.waitForTimeout(INITIAL_PAINT_MS)
  await page.screenshot({ path: join(OUT, 'empty.png') })

  for (const scenario of SCENARIOS) {
    await sendPrompt(page, scenario)
    await waitForBusyIfAny(page)
    await answerPermissions(page, scenario)
    await page.waitForTimeout(SETTLE_MS)
    await page.screenshot({ path: join(OUT, `${scenario}.png`) })
    const gap = await gapToEnd(page)
    if (gap > END_TOLERANCE_PX) problems.push(`${scenario}: ${gap}px short of the end`)
    const easing = await stillEasing(page)
    if (easing > 0) problems.push(`${scenario}: ${easing} block(s) still easing their height`)
    if (scenario === 'markdown') await checkHighlighting(page, problems)
    console.log(`scenario ${scenario}: done`)
  }

  await checkScrolling(page, problems)
  await checkUndo(page, problems)
  await page.click('button[aria-label="Switch to dark theme"]')
  await page.waitForTimeout(THEME_SWITCH_MS)
  await page.screenshot({ path: join(OUT, 'dark.png') })
}

const passed = await withMockPage(async (page, problems, { restartServer }) => {
  await scenarios(page, problems)
  await shellScenario(page, problems)
  await splitComposerScenario(page, problems)
  await splitFocusScenario(page, problems)
  await sidebarTodoScenario(page, problems)
  await sidebarSizingScenario(page, problems)
  await loginLostScenario(page, problems, restartServer)
})
console.log(passed ? `smoke passed, screenshots in ${OUT}` : 'smoke found problems')
process.exit(passed ? 0 : 1)
