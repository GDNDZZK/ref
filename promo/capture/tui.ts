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

async function still(browser: Browser, output: string, name: string, readme: boolean): Promise<void> {
  const png = await renderTerminalStill(browser, { cols: TUI_COLS, rows: TUI_ROWS }, output)
  await publishImage(png, name, { readme })
  await publishWithoutFooter(name)
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
    await still(browser, tui.outputSoFar(), 'tui-split', true)
  })
}

export async function captureTui(): Promise<void> {
  const browser = await launchChromium()
  try {
    await captureTools(browser)
    await captureSplit(browser)
  } finally {
    await browser.close()
  }
}
