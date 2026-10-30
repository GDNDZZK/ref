// Renders scenes to MP4: a headless Chromium seeks the page to each frame time and
// screenshots it, ffmpeg encodes the frames. See promo/README.md.
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Browser, CDPSession, Page } from 'playwright-core'
import { createServer } from 'vite'
import type { Lang } from '../src/engine/i18n.ts'
import { FRAME_HEIGHT, FRAME_WIDTH } from '../src/engine/player.ts'
import { launchChromium } from './lib/browser.ts'
import { parseRenderArgs, type RenderArgs } from './lib/cli.ts'
import { Encoder } from './lib/encoder.ts'

const PAGE_READY_TIMEOUT_MS = 30_000
const PROGRESS_STEPS = 10

// Errors the page reported; checked between batches so a broken scene stops the render.
function throwIfProblems(problems: string[]): void {
  if (problems.length > 0) throw new Error(`the scene page reported errors:\n${problems.join('\n')}`)
}

// One page of the scene in its own browser process, plus a DevTools session for fast screenshots.
interface FramePage {
  page: Page
  devtools: CDPSession
}

async function openScenePage(
  browser: Browser,
  baseUrl: string,
  sceneId: string,
  lang: Lang,
  problems: string[],
): Promise<FramePage> {
  const page = await browser.newPage({ viewport: { width: FRAME_WIDTH, height: FRAME_HEIGHT } })
  page.on('pageerror', (error) => problems.push(error.message))
  page.on('console', (message) => {
    if (message.type() === 'error') problems.push(`console: ${message.text()}`)
  })
  await page.goto(`${baseUrl}/?scene=${sceneId}&lang=${lang}`)
  await page.waitForFunction(() => window.promo !== undefined, undefined, { timeout: PAGE_READY_TIMEOUT_MS })
  return { page, devtools: await page.context().newCDPSession(page) }
}

// Playwright's own screenshot is about 3x slower: it spends the time on PNG compression.
async function screenshotAt({ page, devtools }: FramePage, time: number): Promise<Buffer> {
  await page.evaluate((seconds) => window.promo!.seek(seconds), time)
  const { data } = await devtools.send('Page.captureScreenshot', { format: 'png', optimizeForSpeed: true })
  return Buffer.from(data, 'base64')
}

// Renders frames in batches of one per page. The next batch renders while the previous one
// is written to ffmpeg.
async function renderVideo(
  pages: FramePage[],
  frameCount: number,
  fps: number,
  encoder: Encoder,
  label: string,
  problems: string[],
) {
  const renderBatch = (first: number) =>
    Promise.all(
      pages.slice(0, frameCount - first).map((page, offset) => screenshotAt(page, (first + offset) / fps)),
    )

  const progressEvery = Math.ceil(frameCount / PROGRESS_STEPS)
  let pending = renderBatch(0)
  for (let first = 0; first < frameCount; first += pages.length) {
    const frames = await pending
    throwIfProblems(problems)
    const nextFirst = first + pages.length
    if (nextFirst < frameCount) pending = renderBatch(nextFirst)
    for (const frame of frames) await encoder.write(frame)
    if (Math.floor(first / progressEvery) !== Math.floor(nextFirst / progressEvery)) {
      console.log(`${label}: ${Math.min(nextFirst, frameCount)}/${frameCount} frames`)
    }
  }
}

async function renderScene(browsers: Browser[], baseUrl: string, args: RenderArgs, sceneId: string, lang: Lang) {
  const label = `${sceneId}.${lang}`
  const problems: string[] = []
  const pages = await Promise.all(
    browsers.map((browser) => openScenePage(browser, baseUrl, sceneId, lang, problems)),
  )
  try {
    const duration = await pages[0]!.page.evaluate(() => window.promo!.duration)

    if (args.stillAt !== undefined) {
      const file = join(args.outDir, `${label}.${args.stillAt}s.png`)
      writeFileSync(file, await screenshotAt(pages[0]!, args.stillAt))
      throwIfProblems(problems)
      console.log(`wrote ${file}`)
      return
    }

    const frameCount = Math.round(duration * args.fps)
    const output = join(args.outDir, `${label}.mp4`)
    const encoder = new Encoder({ output, fps: args.fps })
    try {
      await renderVideo(pages, frameCount, args.fps, encoder, label, problems)
      await encoder.finish()
    } catch (error) {
      encoder.abort()
      throw error
    }
    console.log(`wrote ${output} (${duration.toFixed(2)} s, ${frameCount} frames)`)
  } finally {
    await Promise.all(pages.map(({ page }) => page.close()))
  }
}

async function main() {
  const args = parseRenderArgs(process.argv.slice(2))
  mkdirSync(args.outDir, { recursive: true })

  const server = await createServer({ logLevel: 'warn', server: { port: 0 } })
  await server.listen()
  const baseUrl = server.resolvedUrls!.local[0]!.replace(/\/$/, '')
  // One browser process per worker: pages of one browser share a compositor and do not scale.
  const browsers = await Promise.all(Array.from({ length: args.workers }, launchChromium))
  try {
    for (const sceneId of args.sceneIds) {
      for (const lang of args.langs) await renderScene(browsers, baseUrl, args, sceneId, lang)
    }
  } finally {
    await Promise.all(browsers.map((browser) => browser.close()))
    await server.close()
  }
}

await main()
