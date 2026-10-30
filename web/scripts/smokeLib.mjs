// Shared by the headless-Chromium checks (smoke.mjs, perf.mjs): a private `ref-web --mock` on
// temporary state directories, and a browser logged in to it.
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, mkdirSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { chromium } from 'playwright-core'

const REF_WEB = process.env.REF_WEB_BIN ?? '../target/debug/ref-web'
const CHROMIUM = process.env.CHROMIUM ?? '/usr/bin/chromium'
const PORT = Number(process.env.SMOKE_PORT ?? 17480)
const TOKEN_ATTEMPTS = 100
const TOKEN_RETRY_MS = 100

export const OUT = process.env.SMOKE_OUT ?? '/tmp/ref-web-smoke'
export const WAIT_MS = 15000

async function readToken(root) {
  const file = join(root, 'state', 'web', 'login-token')
  for (let attempt = 0; attempt < TOKEN_ATTEMPTS; attempt++) {
    try {
      return readFileSync(file, 'utf8').trim()
    } catch {
      await new Promise((resolve) => setTimeout(resolve, TOKEN_RETRY_MS))
    }
  }
  throw new Error(`no login token at ${file}`)
}

// Runs `body(page, problems, { restartServer })`; reports console errors, failed requests and exceptions as
// problems. Stops only the processes it started. Returns true when there were no problems.
async function waitForPort() {
  for (let attempt = 0; attempt < TOKEN_ATTEMPTS; attempt++) {
    try {
      await fetch(`http://127.0.0.1:${PORT}/api/ping`)
      return
    } catch {
      await new Promise((resolve) => setTimeout(resolve, TOKEN_RETRY_MS))
    }
  }
  throw new Error(`ref-web did not come back on port ${PORT}`)
}

export async function withMockPage(body) {
  const root = mkdtempSync(join(tmpdir(), 'ref-web-smoke-'))
  mkdirSync(OUT, { recursive: true })
  const start = () =>
    spawn(REF_WEB, ['--mock', '--no-open', '--port', String(PORT)], {
      env: { ...process.env, REF_STATE_DIR: join(root, 'state'), REF_DATA_DIR: join(root, 'data') },
      stdio: ['pipe', 'inherit', 'inherit'], // stdin stays open: closing it stops the mock backend
    })
  let server = start()
  // Stops the ref-web started above and starts another one: the browser's login is gone then.
  const restartServer = async () => {
    server.kill() // only the process started above
    await once(server, 'exit')
    server = start()
    await readToken(root)
    await waitForPort()
  }
  const problems = []
  try {
    const token = await readToken(root)
    const browser = await chromium.launch({ executablePath: CHROMIUM, headless: true })
    try {
      const page = await browser.newPage({ viewport: { width: 980, height: 760 }, locale: 'en-US' })
      page.on('console', (message) => {
        if (['error', 'warning'].includes(message.type()))
          problems.push(`console ${message.type()}: ${message.text()}`)
      })
      page.on('response', (response) => {
        if (response.status() >= 400) problems.push(`HTTP ${response.status()}: ${response.url()}`)
      })
      page.on('pageerror', (error) => problems.push(`page error: ${error.message}`))
      await page.goto(`http://127.0.0.1:${PORT}/login?token=${token}`)
      await page.waitForSelector('textarea', { timeout: WAIT_MS })
      await body(page, problems, { restartServer })
    } finally {
      await browser.close()
    }
  } catch (error) {
    problems.push(`failed: ${error instanceof Error ? error.message : error}`)
  } finally {
    server.kill() // only the process started above
    rmSync(root, { recursive: true, force: true })
  }
  for (const problem of problems) console.error(problem)
  return problems.length === 0
}
