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

// Wide enough for the session sidebar (todos, live sessions): it needs 140 columns.
export const TUI_COLS = 150
export const TUI_ROWS = 40
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

// M3 `longrun` for C8, on a mock backend that keeps running without clients (like one the user
// started). The first `ref` sends the prompt and quits at step 6/20; a second `ref` started
// about two minutes later opens the still running session from the sessions dialog and shows
// the output of the gap, until step 18/20 prints live.
// tui-longrun markers: typed, enter, step6, close. tui-longrun-reopen: ready, list, opened, step18.
const LONGRUN_PROMPT = 'longrun run every test in src/'
const LONGRUN_SESSION = 'tests'
const DETAILS_KEYS = '\x18d' // ctrl+x d: tool cards show their live output
const SESSIONS_KEYS = '\x18l' // ctrl+x l
const STEP_SECONDS = 10
// From step 6 to the second start: the session is opened just before step 18 prints.
const REOPEN_AFTER_STEP6_MS = 111_000
const LONGRUN_STEP_TIMEOUT_MS = 3 * STEP_SECONDS * 1000

async function captureLongrun(): Promise<void> {
  const isolated = isolatedEnv()
  const backend = await startMockBackend(isolated, { standalone: true })
  const size = { cols: TUI_COLS, rows: TUI_ROWS }
  try {
    const first = new TuiSession(size, backend, isolated)
    try {
      await first.waitForText('Build ·')
      await first.pause(800)
      first.mark('typed')
      await first.type(LONGRUN_PROMPT, TYPING_DELAY_MS)
      first.mark('enter')
      first.send(KEYS.enter)
      await first.pause(1500)
      first.send(DETAILS_KEYS)
      await first.pause(500)
      await first.type(`/rename ${LONGRUN_SESSION}`, TYPING_DELAY_MS)
      first.send(KEYS.enter)
      await first.waitForText('step 6/20', LONGRUN_STEP_TIMEOUT_MS * 3)
      first.mark('step6')
      await first.pause(1000)
      first.mark('close')
      mkdirSync(CASTS_DIR, { recursive: true })
      first.saveCast(join(CASTS_DIR, 'tui-longrun.cast'))
    } finally {
      await first.close()
    }
    await sleep(REOPEN_AFTER_STEP6_MS - 1000)

    const second = new TuiSession(size, backend, isolated)
    try {
      await second.waitForText('Build ·')
      second.mark('ready')
      await second.pause(1200)
      second.send(DETAILS_KEYS)
      await second.pause(1000)
      second.mark('list')
      second.send(SESSIONS_KEYS)
      await second.pause(1000)
      await second.type(LONGRUN_SESSION, TYPING_DELAY_MS * 2)
      await second.pause(800)
      second.mark('opened')
      second.send(KEYS.enter)
      await second.waitForText('step 18/20', LONGRUN_STEP_TIMEOUT_MS)
      second.mark('step18')
      console.log(`reopened at step 18:\n${second.screenText()}`)
      await second.pause(2000)
      second.saveCast(join(CASTS_DIR, 'tui-longrun-reopen.cast'))
    } finally {
      await second.close()
    }
  } finally {
    await backend.stop()
    isolated.remove()
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

// TUI captures by name, for `pnpm capture tui --only name`.
export const TUI_CAPTURES: Record<string, (browser: Browser) => Promise<void>> = {
  tools: captureTools,
  split: captureSplit,
  outage: captureOutage,
  longrun: captureLongrun,
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
