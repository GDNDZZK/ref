import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Browser } from 'playwright-core'
import { TERMINAL_FONT, TERMINAL_THEME } from '../../src/components/terminalTheme.ts'

const XTERM_DIR = join(import.meta.dirname, '..', '..', 'node_modules', '@xterm', 'xterm')
const FONT_SIZE_PX = 15
const PADDING_PX = 18
const DEVICE_SCALE_FACTOR = 2

// Draws what a terminal of `cols` x `rows` shows after receiving `output`, with the same
// xterm.js and theme as the video scenes. Returns a PNG.
export async function renderTerminalStill(
  browser: Browser,
  size: { cols: number; rows: number },
  output: string,
): Promise<Buffer> {
  const page = await browser.newPage({ deviceScaleFactor: DEVICE_SCALE_FACTOR })
  try {
    await page.setContent(
      `<style>${readFileSync(join(XTERM_DIR, 'css', 'xterm.css'), 'utf8')}
        body { margin: 0; background: ${TERMINAL_THEME.background}; }
        #frame { display: inline-block; padding: ${PADDING_PX}px; background: ${TERMINAL_THEME.background}; }
      </style><div id="frame"><div id="terminal"></div></div>`,
    )
    await page.addScriptTag({ content: readFileSync(join(XTERM_DIR, 'lib', 'xterm.js'), 'utf8') })
    await page.evaluate(
      async ({ cols, rows, theme, fontFamily, fontSize, data }) => {
        const { Terminal } = window as unknown as { Terminal: new (options: object) => any }
        const terminal = new Terminal({ cols, rows, theme, fontFamily, fontSize, cursorBlink: false })
        terminal.open(document.getElementById('terminal'))
        await new Promise<void>((resolve) => terminal.write(data, resolve))
        await new Promise((resolve) => requestAnimationFrame(resolve))
      },
      { ...size, theme: TERMINAL_THEME, fontFamily: TERMINAL_FONT, fontSize: FONT_SIZE_PX, data: output },
    )
    return await page.locator('#frame').screenshot({ type: 'png' })
  } finally {
    await page.close()
  }
}
