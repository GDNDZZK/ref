// Records the TUI against the mock backend: stills for READMEs and videos, and asciicasts for
// the terminal windows in the films.
import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import type { Browser } from 'playwright-core'
import { launchChromium } from '../scripts/lib/browser.ts'
import { isolatedEnv, startMockBackend } from './lib/backend.ts'
import { publishImage, publishWithoutFooter } from './lib/publish.ts'
import { renderTerminalStill } from './lib/terminalStill.ts'
import { KEYS, TuiSession } from './lib/tuiSession.ts'

export const TUI_COLS = 110
export const TUI_ROWS = 32
const CASTS_DIR = join(import.meta.dirname, '..', 'assets', 'casts')
const TYPING_DELAY_MS = 55
const DIALOG_SETTLE_MS = 800 // the permission dialog ignores keys for a moment after it opens
const NEW_SESSION_KEYS = '\x18n' // ctrl+x n
const SPLIT_KEYS = '\x18w' // ctrl+x w

async function withTui(body: (tui: TuiSession) => Promise<void>): Promise<void> {
  const isolated = isolatedEnv()
  const backend = await startMockBackend(isolated)
  const tui = new TuiSession({ cols: TUI_COLS, rows: TUI_ROWS }, backend, isolated)
  try {
    await tui.waitForText('Build ·')
    await tui.pause(800)
    await body(tui)
  } finally {
    await tui.close()
    await backend.stop()
    isolated.remove()
  }
}

// `pathRow`: the last row is the status line with the random mock workspace path (not in the split view).
async function still(browser: Browser, output: string, name: string, readme: boolean, pathRow = true): Promise<void> {
  const png = await renderTerminalStill(browser, { cols: TUI_COLS, rows: TUI_ROWS }, output)
  if (!pathRow) return publishImage(png, name, { readme })
  // The README shows the copy without the status line.
  await publishImage(png, name, { readme: false })
  await publishWithoutFooter(name, { readme })
}

// The mock `tools` script: reads, a failing read, grep, find, an edit that needs approval, bash.
async function captureTools(browser: Browser): Promise<void> {
  await withTui(async (tui) => {
    await tui.type('tools', TYPING_DELAY_MS)
    tui.send(KEYS.enter)

    await tui.waitForText('Permission required')
    await tui.pause(DIALOG_SETTLE_MS)
    const permission = tui.outputSoFar()
    tui.send('a')

    await tui.waitForText('Done.')
    await tui.pause(DIALOG_SETTLE_MS)
    const done = tui.outputSoFar()

    mkdirSync(CASTS_DIR, { recursive: true })
    tui.saveCast(join(CASTS_DIR, 'tui-tools.cast'))
    await still(browser, permission, 'tui-permission', true)
    await still(browser, done, 'tui-main', true)
  })
}

// Four sessions working, then the split view.
async function captureSplit(browser: Browser): Promise<void> {
  await withTui(async (tui) => {
    for (const prompt of ['todo', 'running', 'article']) {
      await tui.type(prompt, TYPING_DELAY_MS)
      tui.send(KEYS.enter)
      await tui.pause(1200)
      tui.send(NEW_SESSION_KEYS)
      await tui.pause(1200)
    }
    await tui.type('slow', TYPING_DELAY_MS)
    tui.send(KEYS.enter)
    await tui.pause(2000)
    tui.send(SPLIT_KEYS)
    await tui.waitForText('background sessions')
    await tui.pause(2500)

    mkdirSync(CASTS_DIR, { recursive: true })
    tui.saveCast(join(CASTS_DIR, 'tui-split.cast'))
    await still(browser, tui.outputSoFar(), 'tui-split', true, false)
  })
}

// M5 `outage`: two tools, four failed requests with growing waits, then the run goes on. A second
// run of the same prompt is stopped with esc esc while it waits. Markers: enter, attempt1..4,
// done, again, esc.
const OUTAGE_PROMPT = 'outage run the tests and fix the failures'
const OUTAGE_DONE = 'All 12 tests pass now.'
const OUTAGE_ATTEMPTS = 4
const OUTAGE_WAIT_MS = 90_000
const ESC = '\x1b'
const DOUBLE_ESC_GAP_MS = 150
const BEFORE_ESC_MS = 1200

async function captureOutage(): Promise<void> {
  await withTui(async (tui) => {
    await tui.type(OUTAGE_PROMPT, TYPING_DELAY_MS)
    tui.mark('enter')
    tui.send(KEYS.enter)
    for (let attempt = 1; attempt <= OUTAGE_ATTEMPTS; attempt++) {
      await tui.waitForText(`(attempt ${attempt})`, OUTAGE_WAIT_MS)
      tui.mark(`attempt${attempt}`)
    }
    await tui.waitForText(OUTAGE_DONE, OUTAGE_WAIT_MS)
    tui.mark('done')
    await tui.pause(1500)

    await tui.type(OUTAGE_PROMPT, TYPING_DELAY_MS)
    tui.mark('again')
    tui.send(KEYS.enter)
    await tui.waitForText('(attempt 1)', OUTAGE_WAIT_MS)
    await tui.pause(BEFORE_ESC_MS)
    tui.mark('esc')
    tui.send(ESC)
    await tui.pause(DOUBLE_ESC_GAP_MS)
    tui.send(ESC)
    await tui.pause(2500)

    mkdirSync(CASTS_DIR, { recursive: true })
    tui.saveCast(join(CASTS_DIR, 'tui-outage.cast'))
  })
}

// TUI captures by name, for `pnpm capture tui --only name`.
export const TUI_CAPTURES: Record<string, (browser: Browser) => Promise<void>> = {
  tools: captureTools,
  split: captureSplit,
  outage: captureOutage,
}

export async function captureTui(only?: readonly string[]): Promise<void> {
  const browser = await launchChromium()
  try {
    for (const [name, capture] of Object.entries(TUI_CAPTURES)) {
      if (!only || only.includes(name)) await capture(browser)
    }
  } finally {
    await browser.close()
  }
}
