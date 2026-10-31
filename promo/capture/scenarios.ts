// Every picture of the interfaces comes from one of these scripts, run against the mock
// backend (docs/development.md#the-mock-backend). Texts waited for are lines the mock prints, so
// they are the same in every interface language.
import type { WebUi } from './lib/webUi.ts'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { REPO_ROOT } from './lib/binaries.ts'
import { shotFileName, type Scenario } from './lib/ui.ts'

const AFTER_CARD_MS = 1200 // a permission card ignores keys for a moment
const STREAM_MS = 5000
const DELAY_RING_MS = 3000 // the text, a temporary file and the delete call take about 2 s
const TOOLS_DONE = 'Done. The edit went through'
const GUI_README_WIDTH = 1280
const GUI_README_HEIGHT = 1344
const LOGO_PNG = join(REPO_ROOT, 'assets', 'app', 'png', 'ref-256.png')

export const SCENARIOS: Scenario[] = [
  {
    name: 'running',
    async run(ui) {
      await ui.send('slow')
      await ui.pause(STREAM_MS)
      await ui.shot('session-running')
    },
  },
  {
    name: 'tools',
    async run(ui) {
      await ui.send('tools')
      await ui.waitText(ui.text('status.waitingPermission'))
      await ui.pause(AFTER_CARD_MS)
      await ui.shot('tools-permission')
      await ui.key('a')
      await ui.waitText(TOOLS_DONE)
      await ui.shot('main', { readme: true })
      await ui.clickText('Fix the off-by-one')
      await ui.pause(400)
      await ui.shot('edit-diff')
    },
  },
  {
    name: 'permission',
    async run(ui) {
      await ui.send('permission')
      await ui.waitText(ui.text('status.waitingPermission'))
      await ui.pause(AFTER_CARD_MS)
      await ui.shot('permission-card', { readme: true })
    },
  },
  {
    name: 'delay',
    async run(ui) {
      await ui.send('delay')
      await ui.pause(DELAY_RING_MS)
      await ui.shot('delay-ring')
      await ui.key('Escape')
      await ui.pause(600)
      await ui.shot('delay-cancelled')
    },
  },
  {
    name: 'denied',
    async run(ui) {
      await ui.send('denied')
      await ui.waitText('refused by the configuration')
      await ui.shot('denied')
    },
  },
  {
    name: 'sessions',
    async run(ui) {
      await ui.send('todo')
      await ui.command('new')
      await ui.pause(500)
      await ui.send('running')
      await ui.command('new')
      await ui.pause(500)
      await ui.send('article')
      await ui.command('new')
      await ui.pause(500)
      await ui.send('slow')
      await ui.pause(4500)
      await ui.shot('sidebar-progress')
      await ui.command('watch')
      await ui.pause(1500)
      await ui.shot('split-view', { readme: true })
      await ui.command('zoom')
      await ui.pause(1200)
      await ui.shot('split-zoom')
    },
  },
  {
    // The README still of the split view. Every tile is finished or sits in a running tool call,
    // so no streaming caret is mid-chase, and the window is tall enough for the todo card to show.
    name: 'readme-split',
    async run(ui) {
      if (ui.kind === 'gui') await ui.resize(GUI_README_WIDTH, GUI_README_HEIGHT)
      await ui.send('todo')
      await ui.command('new')
      await ui.pause(500)
      await ui.send('running')
      await ui.command('new')
      await ui.pause(500)
      await ui.send('denied')
      await ui.command('new')
      await ui.pause(500)
      await ui.send('hello')
      await ui.pause(800)
      await ui.command('watch')
      await ui.pause(2500)
      await ui.shot('split-readme', { readme: true, readmeName: shotFileName(ui.kind, 'split-view', ui.lang) })
    },
  },
  {
    name: 'peers',
    async run(ui) {
      await ui.send('peer')
      await ui.waitText('Review done', 30_000)
      await ui.pause(800)
      await ui.shot('peer-message')
      await ui.send('/peers')
      await ui.pause(1200)
      await ui.shot('peers-dialog')
      await ui.key('Escape')
      await ui.send('/peers-inbox')
      await ui.pause(1200)
      await ui.shot('peers-inbox')
    },
  },
  {
    name: 'composer',
    async run(ui) {
      await ui.send('hello')
      await ui.waitText('Hello! This is the mock backend')
      await ui.type('/co')
      await ui.pause(800)
      await ui.shot('composer-slash')
      await ui.clearPrompt()
      await ui.type('@pa')
      await ui.pause(1000)
      await ui.shot('composer-at')
      await ui.clearPrompt()
      await ui.type('!ls')
      await ui.pause(600)
      await ui.shot('composer-shell')
      await ui.key('Enter')
      await ui.pause(1500)
      await ui.shot('composer-shell-output')
    },
  },
  {
    name: 'queue',
    async run(ui) {
      await ui.send('slow')
      await ui.pause(2500)
      await ui.send('Then run the tests')
      await ui.pause(800)
      await ui.shot('composer-queue')
    },
  },
  {
    name: 'undo',
    async run(ui) {
      await ui.send('hello')
      await ui.waitText('Hello! This is the mock backend')
      await ui.pause(600)
      await ui.shot('before-undo')
      await ui.command('undo')
      await ui.pause(900)
      await ui.shot('composer-undo')
    },
  },
  {
    name: 'image',
    uis: ['web'],
    async run(ui) {
      // Pasting is how the page takes images from the clipboard.
      const page = (ui as WebUi).playwrightPage
      const logo = readFileSync(LOGO_PNG).toString('base64')
      await page.locator('textarea').evaluate((textarea, base64) => {
        const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0))
        const data = new DataTransfer()
        data.items.add(new File([bytes], 'logo.png', { type: 'image/png' }))
        textarea.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }))
      }, logo)
      await ui.pause(600)
      await ui.type('What is in this picture?')
      await ui.shot('composer-image')
      await ui.key('Enter')
      await ui.pause(2500)
      await ui.shot('image-sent')
    },
  },
  {
    name: 'compact',
    async run(ui) {
      await ui.send('fill')
      await ui.waitText('almost full')
      await ui.pause(600)
      await ui.shot('context-full')
      await ui.send('go on')
      await ui.waitText('default reply', 30_000)
      await ui.pause(800)
      await ui.shot('context-compact')
    },
  },
  {
    name: 'retry',
    async run(ui) {
      await ui.send('retry')
      await ui.pause(2200)
      await ui.shot('retry-status')
      await ui.waitText('answered on attempt', 30_000)
      await ui.shot('retry-done')
    },
  },
  {
    name: 'long',
    async run(ui) {
      await ui.send('long')
      await ui.pause(3500)
      await ui.shot('long-output')
    },
  },
  {
    name: 'setup',
    async run(ui) {
      await ui.send('/models')
      await ui.pause(1000)
      await ui.shot('models-dialog')
      await ui.key('Escape')
      await ui.send('/mcp')
      await ui.pause(1000)
      await ui.shot('mcp-dialog')
      await ui.key('Escape')
      await ui.send('/agents')
      await ui.pause(1000)
      await ui.shot('agents-dialog')
    },
  },
  {
    name: 'settings',
    async run(ui) {
      await ui.command('settings')
      await ui.pause(1000)
      await ui.shot('settings')
    },
  },
  {
    name: 'help',
    async run(ui) {
      await ui.command('help')
      await ui.pause(1000)
      await ui.shot('help')
    },
  },
]
