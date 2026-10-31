import { execFileSync } from 'node:child_process'
import { chromium, type Browser } from 'playwright-core'

// System browsers tried when CHROMIUM is not set. The committed films were rendered with the
// system Chromium, and Playwright's own build must match the playwright-core version exactly
// (a newer playwright-core asks for a browser build that may not be installed).
const SYSTEM_BROWSERS = ['chromium', 'chromium-browser', 'google-chrome-stable', 'google-chrome']

function systemBrowser(): string | undefined {
  for (const name of SYSTEM_BROWSERS) {
    try {
      return execFileSync('which', [name], { encoding: 'utf8' }).trim()
    } catch {
      // not installed under this name
    }
  }
  return undefined
}

// CHROMIUM overrides the browser; otherwise a system Chromium, otherwise Playwright's own
// (`pnpm exec playwright-core install chromium` fetches it).
export function launchChromium(): Promise<Browser> {
  return chromium.launch({
    executablePath: process.env.CHROMIUM ?? systemBrowser(),
    headless: true,
    // Same glyph positions on every machine and run.
    args: ['--font-render-hinting=none', '--force-device-scale-factor=1'],
  })
}
