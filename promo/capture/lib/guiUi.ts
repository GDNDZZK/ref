import { rmSync } from 'node:fs'
import { join } from 'node:path'
import type { Lang } from '../../src/engine/i18n.ts'
import { isolatedEnv, startMockBackend } from './backend.ts'
import { CLIP_FPS, CLIP_FRAMES_DIR, saveClip } from './clips.ts'
import { GuiSession } from './guiSession.ts'
import { phrase } from './phrases.ts'
import { publishImage } from './publish.ts'
import { COMMAND_LETTERS, shotFileName, type CommandId, type ShotOptions, type Ui } from './ui.ts'

const WINDOW = { width: 1280, height: 800 }
const MODEL_TAG = 'Mock 1'
const SETTLE_MS = 700 // entrance animations last up to 480 ms
const SIDEBAR_WIDTH = 300
const CONTEXT_METER = /^[\d.]+k? \/ [\d.]+k$/
const TILE_HEADER_INSET = 40 // from the tile's left edge to its title
const SAME_ROW_PX = 40
const STATUS_LABEL_INSET = 12 // from the tile's title to inside its status label
const PARKED_POINTER = { x: 700, y: 20 } // the empty top bar
const GRAB_DIR = join(import.meta.dirname, '..', '..', '.cache', 'grab')

function qtChord(chord: string): string {
  return chord === 'Enter' ? 'Return' : chord
}

class GuiUi implements Ui {
  readonly kind = 'gui'
  readonly lang: Lang
  private readonly session: GuiSession
  private clipName: string | undefined
  private clipDir: string | undefined

  constructor(session: GuiSession, lang: Lang) {
    this.session = session
    this.lang = lang
  }

  text(key: string): string {
    return phrase(this.lang, key)
  }

  async send(text: string): Promise<void> {
    await this.session.type(text)
    await this.session.key('Return')
  }

  type(text: string): Promise<void> {
    return this.session.type(text)
  }

  async clearPrompt(): Promise<void> {
    await this.session.key('Ctrl+A')
    await this.session.key('Backspace')
  }

  key(chord: string): Promise<void> {
    return this.session.key(qtChord(chord))
  }

  command(id: CommandId): Promise<void> {
    return this.session.key(`Ctrl+Shift+${COMMAND_LETTERS[id]}`)
  }

  clickText(text: string): Promise<void> {
    return this.session.clickText(text)
  }

  async clickTextBelow(text: string, anchor: string): Promise<void> {
    const items = await this.session.texts()
    const top = items.find((item) => item.text.includes(anchor))?.y
    if (top === undefined) throw new Error(`no visible text "${anchor}"`)
    const target = items.filter((item) => item.text.includes(text) && item.y >= top).sort((a, b) => a.y - b.y)[0]
    if (!target) throw new Error(`no visible text "${text}" below "${anchor}"`)
    await this.session.click(target.x + target.width / 2, target.y + target.height / 2)
  }

  parkPointer(): Promise<void> {
    return this.session.hover(PARKED_POINTER.x, PARKED_POINTER.y)
  }

  // A click on the header or the context meter does not focus a tile, and a click on a tool card
  // opens it, so this clicks the status label ("Working") at the left end of the tile's status
  // line, which is found by its context meter ("5.0k / 200.0k") at the right end.
  async clickTile(title: string): Promise<void> {
    const texts = await this.session.texts()
    const header = texts.find((item) => item.text === title && item.x >= SIDEBAR_WIDTH)
    if (!header) throw new Error(`no tile "${title}"`)
    const meters = texts.filter((item) => CONTEXT_METER.test(item.text) && item.y > header.y && item.x >= header.x - TILE_HEADER_INSET)
    const top = Math.min(...meters.map((item) => item.y))
    const meter = meters.filter((item) => item.y - top < SAME_ROW_PX).sort((a, b) => a.x - b.x)[0]
    if (!meter) throw new Error(`no status line in tile "${title}"`)
    await this.session.click(header.x + STATUS_LABEL_INSET, meter.y + meter.height / 2)
  }

  async clickSidebarRow(title: string): Promise<void> {
    const row = (await this.session.texts()).find((item) => item.text === title && item.x < SIDEBAR_WIDTH)
    if (!row) throw new Error(`no sidebar row "${title}"`)
    await this.session.click(row.x + row.width / 2, row.y + row.height / 2)
  }

  hasText(text: string): Promise<boolean> {
    return this.session.hasText(text)
  }

  waitText(text: string, timeoutMs?: number): Promise<void> {
    return this.session.waitForText(text, timeoutMs)
  }

  waitNoText(text: string, timeoutMs?: number): Promise<void> {
    return this.session.waitForNoText(text, timeoutMs)
  }

  async startClip(name: string): Promise<void> {
    this.clipName = shotFileName(this.kind, name, this.lang)
    this.clipDir = join(CLIP_FRAMES_DIR, this.clipName)
    rmSync(this.clipDir, { recursive: true, force: true })
    await this.session.startRecording(this.clipDir, CLIP_FPS)
  }

  async endClip(): Promise<void> {
    if (!this.clipName || !this.clipDir) throw new Error('no clip is being recorded')
    const times = await this.session.stopRecording()
    await saveClip(this.clipName, this.clipDir, times)
    this.clipName = undefined
    this.clipDir = undefined
  }

  async resize(width: number, height: number): Promise<void> {
    await this.session.resize(width, height)
  }

  pause(ms: number): Promise<void> {
    return this.session.pause(ms)
  }

  async shot(name: string, options: ShotOptions = {}): Promise<void> {
    await this.session.pause(SETTLE_MS)
    const fileName = shotFileName(this.kind, name, this.lang)
    const png = await this.session.grab(join(GRAB_DIR, `${fileName}.png`))
    const sessionName = (await this.session.texts()).find((item) => SESSION_NAME.test(item.text))?.text
    await publishImage(png, fileName, { readme: options.readme ?? false, readmeName: options.readmeName, sessionName })
  }
}

// A mock session is named "workspace-" and four hex digits.
const SESSION_NAME = /^workspace-[0-9a-f]{4}$/

export type { GuiUi }

// Runs `body` against a fresh `ref-server --mock` and an offscreen `ref-gui` connected to it.
export async function withGuiUi(lang: Lang, body: (ui: GuiUi) => Promise<void>): Promise<void> {
  const isolated = isolatedEnv()
  const backend = await startMockBackend(isolated)
  const session = new GuiSession({ ...WINDOW, lang, theme: 'dark' }, backend, isolated)
  try {
    await session.waitForWindow()
    await session.waitForText(MODEL_TAG) // the prompt box shows the model once the session is attached
    await body(new GuiUi(session, lang))
  } finally {
    await session.close()
    await backend.stop()
    isolated.remove()
  }
}
