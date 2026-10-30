// Records the TUI against the mock backend: stills for READMEs and videos, and an
// asciicast for the terminal scenes.
import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { launchChromium } from '../scripts/lib/browser.ts'
import { isolatedEnv, startMockBackend } from './lib/backend.ts'
import { publishImage } from './lib/publish.ts'
import { renderTerminalStill } from './lib/terminalStill.ts'
import { KEYS, TuiSession } from './lib/tuiSession.ts'

const COLS = 110
const ROWS = 32
const CASTS_DIR = join(import.meta.dirname, '..', 'assets', 'casts')
const TYPING_DELAY_MS = 55
const DIALOG_SETTLE_MS = 800 // the permission dialog ignores keys for a moment after it opens

// The mock `tools` script: reads, a failing read, grep, find, an edit that needs approval, bash.
async function captureTools(): Promise<void> {
  const isolated = isolatedEnv()
  const backend = await startMockBackend(isolated)
  const tui = new TuiSession({ cols: COLS, rows: ROWS }, backend, isolated)
  const browser = await launchChromium()
  try {
    await tui.waitForText('Build ·')
    await tui.pause(800)
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
    const size = { cols: COLS, rows: ROWS }
    await publishImage(await renderTerminalStill(browser, size, permission), 'tui-permission', { readme: true })
    await publishImage(await renderTerminalStill(browser, size, done), 'tui-tools', { readme: true })
  } finally {
    await browser.close()
    await tui.close()
    await backend.stop()
    isolated.remove()
  }
}

export async function captureTui(): Promise<void> {
  await captureTools()
}
