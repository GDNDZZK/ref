// Every picture of the interfaces comes from one of these scripts, run against the mock
// backend (docs/development.md#the-mock-backend). Texts waited for are lines the mock prints, so
// they are the same in every interface language.
import type { GuiUi } from './lib/guiUi.ts'
import type { WebUi } from './lib/webUi.ts'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { REPO_ROOT } from './lib/binaries.ts'
import { ORCHESTRA_SCENARIOS } from './orchestra.ts'
import { shotFileName, type Scenario, type Ui } from './lib/ui.ts'
import type { Lang } from '../src/engine/i18n.ts'

const AFTER_CARD_MS = 1200 // a permission card ignores keys for a moment
const STREAM_MS = 5000
const DELAY_RING_MS = 3000 // the text, a temporary file and the delete call take about 2 s
const TOOLS_DONE = 'Done. The edit went through'
// Lets the split view redraw after a to-do tick.
const AFTER_TICK_MS = 800
// The README split view: the hero script's crew, after main's second report.
const CREW_MAIN = 'main'
const CREW_TASK = 'orchestrate: fix the parser bug'
const CREW_TWO_DONE = 'Two helpers are still working.'
const CREW_WAIT_MS = 30_000
const GUI_README_WIDTH = 1280
const GUI_README_HEIGHT = 1344
const LOGO_SVG = join(REPO_ROOT, 'assets', 'app', 'ref.svg')
const LOGO_PIXELS = 256
const STALE_SESSION = 'parser-rename'
const STALE_PROMPT = 'stale rename parse_line to parse_row'
const STALE_WAIT_MS = 30_000
const STALE_HOLD_MS = 2500
// Texts of the `stale` turn, in order, and the clip mark set when each shows up.
const STALE_MOMENTS: [string, string][] = [
  ['read', 'Meanwhile the file changes on disk'],
  ['pulled', '3 lines added'],
  ['moved', 'Now the library.'],
  ['changed', 'line 12 changed'],
  ['rejected', 'The edit was rejected.'],
  ['retried', 'Now the edit uses the current lines.'],
  ['done', 'Done. The first edit was moved'],
]
// The cards the clip opens after the turn, by their purpose (the `nth` card with that purpose), and
// the end of the conversation they are near.
const STALE_CARDS: { mark: string; purpose: string; nth: number; end: 'top' | 'bottom' }[] = [
  { mark: 'readOpen', purpose: 'Read the parser', nth: 0, end: 'top' },
  { mark: 'pullOpen', purpose: 'Add three lines at the top', nth: 0, end: 'top' },
  { mark: 'movedOpen', purpose: 'Rename in the parser', nth: 0, end: 'top' },
  { mark: 'rejectedOpen', purpose: 'Rename in the library', nth: 0, end: 'bottom' },
  { mark: 'retriedOpen', purpose: 'Rename in the library', nth: 1, end: 'bottom' },
]
const STALE_SCROLL_NOTCHES = 20 // enough to reach either end
const STALE_SCROLL_MS = 900
const STALE_DIFF_NOTCHES = 4
const STALE_CARD_MS = 2500
const STALE_CLOSE_MS = 700
const QUESTION_SESSION = 'parser-fix'
const QUESTION_PROMPT = 'question-one'
const QUESTION_TITLE = '? First file'
const QUESTION_REPLY = 'You chose:' // the mock repeats the question and the answer
const QUESTION_READ_MS = 4500 // the film shows the dialog for about 4.6 s before Enter
const QUESTION_HOLD_MS = 4000
// GUI button and status texts (gui/src/app/PermissionSection.qml, ToolBlock.qml and their
// Chinese translation), where they differ from the web's words.
const GUI_ALLOW_SESSION: Record<Lang, string> = { en: 'Allow for session', zh: '本会话内允许' }
const GUI_RUNS_IN: Record<Lang, string> = { en: 'Runs in', zh: '后执行' }
const CARD_READ_MS = 2500
const COUNTDOWN_WATCHED_MS = 3300
const CLIP_HOLD_MS = 3000
const SESSION_FOR_TAKE = 'parser-fix'
// The `todo` script's first tick; stopped there, its list stays at 1/3 with step two in progress.
const TODO_FIRST_TICK = 'Step one is done'
const BEFORE_STOP_MS = 400
const DOUBLE_ESC_GAP_MS = 150
const AFTER_STOP_MS = 800
const HELLO_REPLY = 'This is the mock backend'

// For README stills: a named session whose sidebar to-do list is in progress (one done, one in
// progress, one pending). The `todo` turn is stopped with esc esc right after its first tick; a
// short `hello` turn after it moves the stopped turn above the visible part of the conversation.
async function leaveTodoInProgress(ui: Ui): Promise<void> {
  await ui.send(`/rename ${SESSION_FOR_TAKE}`)
  await ui.pause(600)
  await ui.send('todo')
  await ui.waitText(TODO_FIRST_TICK)
  await ui.pause(BEFORE_STOP_MS)
  await ui.key('Escape')
  await ui.pause(DOUBLE_ESC_GAP_MS)
  await ui.key('Escape')
  await ui.pause(AFTER_STOP_MS)
  await ui.send('hello')
  await ui.waitText(HELLO_REPLY)
  await ui.pause(AFTER_STOP_MS)
}

// Names the session, types `prompt` and starts the clip `clip`; Enter is the clip mark "enter".
async function startTakeWithPrompt(ui: Ui, prompt: string, clip: string): Promise<void> {
  await ui.send(`/rename ${SESSION_FOR_TAKE}`)
  await ui.pause(600)
  await ui.type(prompt)
  await ui.parkPointer()
  await ui.pause(800)
  await ui.startClip(clip)
  await ui.pause(500)
  await ui.key('Enter')
  ui.markClip('enter')
}

export const SCENARIOS: Scenario[] = [
  ...ORCHESTRA_SCENARIOS,
  {
    name: 'running',
    async run(ui) {
      await ui.send('slow')
      await ui.pause(STREAM_MS)
      await ui.shot('session-running')
    },
  },
  {
    // The README main view, with a to-do list in progress in the sidebar and the status line.
    name: 'tools',
    async run(ui) {
      await leaveTodoInProgress(ui)
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
      await leaveTodoInProgress(ui)
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
    // C5 shot 3: the permission card of an edit; the pointer clicks "Allow for session".
    name: 'permission-clip',
    uis: ['gui'],
    async run(ui) {
      const gui = ui as GuiUi
      await startTakeWithPrompt(ui, 'permission', 'permission')
      await ui.waitText(ui.text('status.waitingPermission'))
      ui.markClip('card')
      await ui.pause(CARD_READ_MS)
      await gui.clickLabel(GUI_ALLOW_SESSION[ui.lang])
      ui.markClip('allowed')
      await ui.parkPointer()
      await ui.pause(CLIP_HOLD_MS)
      await ui.endClip()
    },
  },
  {
    // C5 shot 4 and Film A A11: a delete waits 5 s before it runs; Cancel is clicked 3.3 s
    // into the countdown.
    name: 'delay-clip',
    uis: ['gui'],
    async run(ui) {
      const gui = ui as GuiUi
      await startTakeWithPrompt(ui, 'delay', 'delay')
      await ui.waitText(GUI_RUNS_IN[ui.lang])
      ui.markClip('countdown')
      await ui.pause(COUNTDOWN_WATCHED_MS)
      await gui.clickLabel(ui.text('tool.cancel'))
      ui.markClip('cancel')
      await ui.parkPointer()
      await ui.waitText(ui.text('tool.cancelledState'))
      ui.markClip('cancelled')
      await ui.pause(CLIP_HOLD_MS)
      await ui.endClip()
    },
  },
  {
    name: 'denied',
    async run(ui) {
      await ui.send(`/rename ${SESSION_FOR_TAKE}`)
      await ui.pause(600)
      await ui.send('denied')
      await ui.waitText('refused by the configuration')
      await ui.shot('denied')
    },
  },
  {
    // M4 in the shooting script, for C2: a read, the file changes, the edit is moved; then a
    // second file whose edited lines change, and the edit is rejected and redone. The turn takes
    // about 4 s with every card closed, so afterwards the clip opens the cards the film shows,
    // one at a time. Clip marks name the moments the film cuts on.
    name: 'stale',
    uis: ['gui'],
    async run(ui) {
      const gui = ui as GuiUi
      await ui.send(`/rename ${STALE_SESSION}`)
      await ui.pause(600)
      await ui.type(STALE_PROMPT)
      await ui.parkPointer()
      await ui.pause(800)
      await ui.startClip('stale')
      await ui.pause(500)
      await ui.key('Enter')
      ui.markClip('enter')
      for (const [mark, text] of STALE_MOMENTS) {
        await ui.waitText(text, STALE_WAIT_MS)
        ui.markClip(mark)
      }
      await ui.pause(STALE_HOLD_MS)

      for (const [index, card] of STALE_CARDS.entries()) {
        const previous = STALE_CARDS[index - 1]
        if (card.end !== previous?.end) {
          await gui.scroll(card.end === 'top' ? STALE_SCROLL_NOTCHES : -STALE_SCROLL_NOTCHES)
          await ui.pause(STALE_SCROLL_MS)
        }
        await gui.clickLabel(card.purpose, card.nth)
        ui.markClip(card.mark)
        await ui.parkPointer()
        await ui.pause(STALE_CARD_MS)
        if (index === STALE_CARDS.length - 1) break // the last card stays open
        await gui.clickLabel(card.purpose, card.nth)
        await ui.parkPointer()
        await ui.pause(STALE_CLOSE_MS)
      }
      // The diff of the last card runs past the bottom of the view.
      await gui.scroll(-STALE_DIFF_NOTCHES)
      await ui.pause(STALE_SCROLL_MS)
      ui.markClip('retriedDiff')
      await ui.pause(STALE_CARD_MS)
      await ui.endClip()
    },
  },
  {
    // M6 in the shooting script, for C4: `question-one` asks one question with a default. The
    // GUI records a clip and takes the default with Enter well past the 350 ms guard; ref-web
    // gives the still of its dialog (the films draw the alerting tab).
    name: 'question',
    async run(ui) {
      await ui.send(`/rename ${QUESTION_SESSION}`)
      await ui.pause(600)
      if (ui.kind === 'web') {
        await ui.send(QUESTION_PROMPT)
        await ui.waitText(QUESTION_TITLE)
        await ui.pause(AFTER_CARD_MS)
        await ui.shot('question-alert')
        return
      }
      await ui.type(QUESTION_PROMPT)
      await ui.parkPointer()
      await ui.pause(800)
      await ui.startClip('question')
      await ui.pause(500)
      await ui.key('Enter')
      ui.markClip('enter')
      await ui.waitText(QUESTION_TITLE)
      ui.markClip('dialog')
      await ui.pause(QUESTION_READ_MS)
      await ui.key('Enter')
      ui.markClip('answer')
      await ui.waitText(QUESTION_REPLY)
      ui.markClip('reply')
      await ui.pause(QUESTION_HOLD_MS)
      await ui.endClip()
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
    // The README still of the split view: the crew of the hero script (main and the helpers it
    // names) about 13 s in. main's list in the sidebar is at 2/5 with "Test the fix" in progress,
    // the fixer's edit waits for approval and the tester is still running.
    name: 'readme-split',
    async run(ui) {
      if (ui.kind === 'gui') await ui.resize(GUI_README_WIDTH, GUI_README_HEIGHT)
      await ui.send(`/rename ${CREW_MAIN}`)
      await ui.pause(600)
      await ui.send(CREW_TASK)
      await ui.pause(400)
      await ui.command('watch')
      if (ui.kind === 'gui') await ui.parkPointer()
      await ui.waitText(CREW_TWO_DONE, CREW_WAIT_MS)
      await ui.pause(AFTER_TICK_MS)
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
      // The logo exists only as SVG; the page rasterises it, since a pasted screenshot is a PNG.
      const logo = readFileSync(LOGO_SVG, 'utf8')
      await page.locator('textarea').evaluate(
        async (textarea, { svg, pixels }) => {
          const image = new Image()
          image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
          await image.decode()
          const canvas = document.createElement('canvas')
          canvas.width = pixels
          canvas.height = pixels
          canvas.getContext('2d')!.drawImage(image, 0, 0, pixels, pixels)
          const png = await new Promise<Blob>((resolve, reject) =>
            canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('the logo did not rasterise'))), 'image/png'),
          )
          const data = new DataTransfer()
          data.items.add(new File([png], 'logo.png', { type: 'image/png' }))
          textarea.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }))
        },
        { svg: logo, pixels: LOGO_PIXELS },
      )
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
