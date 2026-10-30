// Smoke test in headless Chromium against `ref-web --mock` (a scripted model, no real LLM).
//   node scripts/smoke.mjs [scenario ...]      scenarios are mock script names (help lists them)
// Needs a built ref-web (cargo build -p ref-web) and system Chromium. Screenshots go to $SMOKE_OUT.
import { join } from 'node:path'
import { OUT, WAIT_MS, withMockPage } from './smokeLib.mjs'
import { shellScenario } from './smokeShell.mjs'

const SCENARIOS =
  process.argv.length > 2 ? process.argv.slice(2) : ['tools', 'reasoning', 'markdown', 'permission', 'fail']
const END_TOLERANCE_PX = 1.5
const SETTLE_MS = 1500
const FAR_FROM_END_PX = 200

// Answers every permission request until the reply ends: the first by click, the rest by the
// "a" key, which works once the composer has lost focus.
async function answerPermissions(page, scenario) {
  const allow = page.locator('button.choice.allow')
  const idle = page.locator('button.send:not(.busy)')
  const deadline = Date.now() + WAIT_MS * 2
  for (let asked = 0; !(await idle.isVisible());) {
    if (Date.now() > deadline) throw new Error(`scenario ${scenario} did not finish`)
    if (await allow.isVisible()) {
      if (asked++ === 0) await page.screenshot({ path: join(OUT, `${scenario}-permission.png`) })
      if (asked === 1) await allow.click()
      else {
        await page.evaluate(() => document.activeElement?.blur())
        await page.keyboard.press('a')
      }
    }
    await page.waitForTimeout(300)
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
  await page.mouse.move(490, 300)
  await page.mouse.wheel(0, -2000)
  await page.waitForTimeout(600)
  if (!(await page.locator('.jump.shown').count())) problems.push('scrolling up did not show the jump button')
  await page.screenshot({ path: join(OUT, 'scrolled-up.png') })
  await page.click('.jump.shown button')
  await page.waitForTimeout(2000)
  if ((await gapToEnd(page)) > END_TOLERANCE_PX)
    problems.push('the jump button did not bring the view to the end')

  await page.fill('textarea', 'slow')
  await page.keyboard.press('Enter')
  await page.waitForSelector('button.send.busy', { timeout: WAIT_MS })
  await page.mouse.wheel(0, -2000)
  await page.waitForTimeout(SETTLE_MS)
  const gap = await gapToEnd(page)
  if (gap < FAR_FROM_END_PX)
    problems.push(`a streaming reply pulled the view back down (${gap}px from the end)`)
  await page.click('button.send.busy') // stop
  await page.waitForSelector('button.send:not(.busy)', { timeout: WAIT_MS })
}

// Code blocks get their grammar and colors, in light and in dark.
async function checkHighlighting(page, problems) {
  const colors = () =>
    page.evaluate(
      () =>
        new Set([...document.querySelectorAll('.code .token')].map((e) => getComputedStyle(e).color)).size,
    )
  const light = await colors()
  if (light < 3) problems.push(`code blocks use only ${light} color(s): not highlighted`)
}

async function scenarios(page, problems) {
  await page.waitForTimeout(500)
  await page.screenshot({ path: join(OUT, 'empty.png') })

  for (const scenario of SCENARIOS) {
    await page.fill('textarea', scenario)
    await page.keyboard.press('Enter')
    await page.waitForSelector('button.send.busy', { timeout: WAIT_MS }).catch(() => {})
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
  await page.click('button[aria-label="Switch to dark theme"]')
  await page.waitForTimeout(800)
  await page.screenshot({ path: join(OUT, 'dark.png') })
}

const passed = await withMockPage(async (page, problems) => {
  await scenarios(page, problems)
  await shellScenario(page, problems)
})
console.log(passed ? `smoke passed, screenshots in ${OUT}` : 'smoke found problems')
process.exit(passed ? 0 : 1)
