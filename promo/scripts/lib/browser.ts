import { chromium, type Browser } from 'playwright-core'

// CHROMIUM overrides the browser; otherwise Playwright's own Chromium is used
// (`pnpm exec playwright-core install chromium` fetches it).
export function launchChromium(): Promise<Browser> {
  return chromium.launch({
    executablePath: process.env.CHROMIUM,
    headless: true,
    // Same glyph positions on every machine and run.
    args: ['--font-render-hinting=none', '--force-device-scale-factor=1'],
  })
}
