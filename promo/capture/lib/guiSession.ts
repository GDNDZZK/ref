import { spawn, type ChildProcess } from 'node:child_process'
import { once } from 'node:events'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import type { Lang } from '../../src/engine/i18n.ts'
import type { Backend, IsolatedEnv } from './backend.ts'
import { REPO_ROOT } from './binaries.ts'

const PROMO_DIR = join(import.meta.dirname, '..', '..')
const GUI_BINARY = process.env.REF_GUI_BIN ?? join(PROMO_DIR, '.cache', 'gui-build', 'src', 'app', 'ref-gui')
// Directory with generic/librefpromo.so, built from capture/gui-driver.
const DRIVER_PLUGIN_DIR = process.env.REF_GUI_DRIVER_DIR ?? join(PROMO_DIR, '.cache', 'gui-driver-build', 'plugins')
// KSyntaxHighlighting's QML module (code highlighting).
const QML_IMPORT_PATH = process.env.REF_GUI_QML_PATH ?? join(REPO_ROOT, 'gui', '.deps', 'prefix', 'lib', 'qml')

const POLL_MS = 50
const WAIT_TIMEOUT_MS = 30_000
const SCALE_FACTOR = 2 // the window is grabbed at twice its size, for zooming in videos
const GUI_LANGUAGE: Record<Lang, string> = { en: 'en', zh: 'zh_CN' }

export interface GuiOptions {
  // Logical window size.
  width: number
  height: number
  lang: Lang
  theme: 'dark' | 'light'
  // Arguments for ref-gui besides --server/--token.
  args?: string[]
}

interface Reply {
  ok: boolean
  error?: string
  [field: string]: unknown
}

export interface TextOnScreen {
  text: string
  x: number
  y: number
  width: number
  height: number
}

// An unmodified `ref-gui` under the offscreen platform, with the capture/gui-driver plugin
// loaded. Commands go over stdin, replies come back on stdout.
export class GuiSession {
  private readonly child: ChildProcess
  private readonly exited: Promise<unknown>
  private readonly replies: ((reply: Reply) => void)[] = []
  private buffered = ''

  constructor(options: GuiOptions, backend: Backend, isolated: IsolatedEnv) {
    mkdirSync(isolated.env.REF_STATE_DIR!, { recursive: true })
    const settings = {
      theme: options.theme,
      language: GUI_LANGUAGE[options.lang],
      window: { width: options.width, height: options.height },
    }
    writeFileSync(join(isolated.env.REF_STATE_DIR!, 'gui.json'), JSON.stringify(settings))

    this.child = spawn(GUI_BINARY, ['--server', backend.url, '--token', backend.token, ...(options.args ?? [])], {
      env: {
        ...isolated.env,
        QT_QPA_PLATFORM: 'offscreen',
        QT_QPA_GENERIC_PLUGINS: 'refpromo',
        QT_PLUGIN_PATH: DRIVER_PLUGIN_DIR,
        QML_IMPORT_PATH,
        QT_SCALE_FACTOR: String(SCALE_FACTOR),
      },
      stdio: ['pipe', 'pipe', 'inherit'],
    })
    this.exited = once(this.child, 'exit')
    this.child.stdout!.setEncoding('utf8')
    this.child.stdout!.on('data', (chunk: string) => this.onOutput(chunk))
  }

  private onOutput(chunk: string): void {
    this.buffered += chunk
    for (let end = this.buffered.indexOf('\n'); end >= 0; end = this.buffered.indexOf('\n')) {
      const line = this.buffered.slice(0, end)
      this.buffered = this.buffered.slice(end + 1)
      this.replies.shift()?.(JSON.parse(line) as Reply)
    }
  }

  private async command(fields: Record<string, unknown>): Promise<Reply> {
    const reply = new Promise<Reply>((resolve) => this.replies.push(resolve))
    this.child.stdin!.write(`${JSON.stringify(fields)}\n`)
    const result = await reply
    if (!result.ok) throw new Error(`${JSON.stringify(fields)} failed: ${result.error}`)
    return result
  }

  // Resolves once the window is on screen.
  async waitForWindow(): Promise<void> {
    const deadline = performance.now() + WAIT_TIMEOUT_MS
    for (;;) {
      const reply = await this.command({ cmd: 'window' }).catch(() => undefined)
      if (reply?.width !== undefined) return
      if (performance.now() > deadline) throw new Error('ref-gui showed no window')
      await sleep(POLL_MS)
    }
  }

  async texts(): Promise<TextOnScreen[]> {
    return ((await this.command({ cmd: 'texts' })).texts as TextOnScreen[]) ?? []
  }

  async hasText(text: string): Promise<boolean> {
    return (await this.texts()).some((item) => item.text.includes(text))
  }

  async waitForText(text: string, timeoutMs = WAIT_TIMEOUT_MS): Promise<void> {
    const deadline = performance.now() + timeoutMs
    while (!(await this.hasText(text))) {
      if (performance.now() > deadline) throw new Error(`timed out waiting for "${text}"; visible: ${(await this.texts()).map((item) => item.text).join(' | ')}`)
      await sleep(POLL_MS)
    }
  }

  async waitForNoText(text: string, timeoutMs = WAIT_TIMEOUT_MS): Promise<void> {
    const deadline = performance.now() + timeoutMs
    while (await this.hasText(text)) {
      if (performance.now() > deadline) throw new Error(`"${text}" stayed visible`)
      await sleep(POLL_MS)
    }
  }

  async clickText(text: string): Promise<void> {
    await this.command({ cmd: 'clickText', text })
  }

  async click(x: number, y: number): Promise<void> {
    await this.command({ cmd: 'click', x, y })
  }

  async hover(x: number, y: number): Promise<void> {
    await this.command({ cmd: 'move', x, y })
  }

  // Qt key sequence syntax: "Return", "Escape", "Ctrl+P", "Ctrl+Shift+N".
  async key(sequence: string): Promise<void> {
    await this.command({ cmd: 'key', sequence })
  }

  async type(text: string): Promise<void> {
    await this.command({ cmd: 'type', text })
  }

  pause(ms: number): Promise<void> {
    return sleep(ms)
  }

  // The window as a PNG, `SCALE_FACTOR` times its logical size.
  async grab(file: string): Promise<Buffer> {
    mkdirSync(dirname(file), { recursive: true })
    await this.command({ cmd: 'grab', file })
    return readFileSync(file)
  }

  // Ends ref-gui through its own quit command, then by PID if it does not go.
  async close(): Promise<void> {
    if (this.child.exitCode !== null) return
    this.child.stdin!.write('{"cmd":"quit"}\n')
    await Promise.race([this.exited, sleep(3000)])
    if (this.child.exitCode === null) {
      this.child.kill()
      await this.exited
    }
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}
