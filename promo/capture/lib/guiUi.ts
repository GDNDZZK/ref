import { join } from 'node:path'
import type { Lang } from '../../src/engine/i18n.ts'
import { isolatedEnv, startMockBackend } from './backend.ts'
import { GuiSession } from './guiSession.ts'
import { phrase } from './phrases.ts'
import { publishImage } from './publish.ts'
import { COMMAND_LETTERS, shotFileName, type CommandId, type ShotOptions, type Ui } from './ui.ts'

const WINDOW = { width: 1280, height: 800 }
const MODEL_TAG = 'Mock 1'
const SETTLE_MS = 700 // entrance animations last up to 480 ms
const GRAB_DIR = join(import.meta.dirname, '..', '..', '.cache', 'grab')

function qtChord(chord: string): string {
  return chord === 'Enter' ? 'Return' : chord
}

class GuiUi implements Ui {
  readonly kind = 'gui'
  readonly lang: Lang
  private readonly session: GuiSession

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

  hasText(text: string): Promise<boolean> {
    return this.session.hasText(text)
  }

  waitText(text: string, timeoutMs?: number): Promise<void> {
    return this.session.waitForText(text, timeoutMs)
  }

  waitNoText(text: string, timeoutMs?: number): Promise<void> {
    return this.session.waitForNoText(text, timeoutMs)
  }

  pause(ms: number): Promise<void> {
    return this.session.pause(ms)
  }

  async shot(name: string, options: ShotOptions = {}): Promise<void> {
    await this.session.pause(SETTLE_MS)
    const fileName = shotFileName(this.kind, name, this.lang)
    const png = await this.session.grab(join(GRAB_DIR, `${fileName}.png`))
    const sessionName = (await this.session.texts()).find((item) => SESSION_NAME.test(item.text))?.text
    await publishImage(png, fileName, { readme: options.readme ?? false, sessionName })
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
