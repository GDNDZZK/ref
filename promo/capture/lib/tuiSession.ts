import { spawn, type ChildProcess } from 'node:child_process'
import { once } from 'node:events'
import { writeFileSync } from 'node:fs'
import xtermHeadless from '@xterm/headless'
import { binary } from './binaries.ts'
import type { Backend, IsolatedEnv } from './backend.ts'

const { Terminal } = xtermHeadless

const WAIT_TIMEOUT_MS = 20_000
const WAIT_POLL_MS = 50
const QUIT_KEYS = '\x18q' // ctrl+x q
const QUIT_GRACE_MS = 1500

export interface TuiOptions {
  cols: number
  rows: number
  // Arguments after the binary name, e.g. ['--agent', 'plan'].
  args?: string[]
}

export const KEYS = {
  enter: '\r',
  escape: '\x1b',
  tab: '\t',
  ctrl: (letter: string) => String.fromCharCode(letter.charCodeAt(0) - 'a'.charCodeAt(0) + 1),
} as const

// `ref` running in a pseudo terminal (util-linux `script`) against a mock backend. Keeps a
// screen model for waiting on text and records everything as asciicast v2 events.
export class TuiSession {
  private readonly child: ChildProcess
  private readonly screen: InstanceType<typeof Terminal>
  private readonly startedAt = performance.now()
  private readonly events: [number, string, string][] = []
  private readonly decoder = new TextDecoder()
  private readonly exited: Promise<unknown>

  private readonly options: TuiOptions

  constructor(options: TuiOptions, backend: Backend, isolated: IsolatedEnv) {
    this.options = options
    this.screen = new Terminal({ cols: options.cols, rows: options.rows, allowProposedApi: true })
    const command =
      `stty cols ${options.cols} rows ${options.rows}; exec ${binary('ref')} ` +
      `--server ${backend.url} --token ${backend.token} ${(options.args ?? []).join(' ')}`
    // -q quiet, -e return the child's exit code, -f flush after each write; the log goes nowhere.
    this.child = spawn('script', ['-qefc', command, '/dev/null'], {
      env: isolated.env,
      stdio: ['pipe', 'pipe', 'inherit'],
    })
    this.exited = once(this.child, 'exit')
    this.child.stdout!.on('data', (chunk: Buffer) => this.onOutput(chunk))
  }

  private onOutput(chunk: Buffer): void {
    const text = this.decoder.decode(chunk, { stream: true })
    this.events.push([(performance.now() - this.startedAt) / 1000, 'o', text])
    this.screen.write(text)
  }

  // The visible screen as text, one line per row, trailing spaces removed.
  screenText(): string {
    const buffer = this.screen.buffer.active
    const rows = Array.from({ length: this.options.rows }, (_, row) =>
      (buffer.getLine(buffer.viewportY + row)?.translateToString(true) ?? '').trimEnd(),
    )
    return rows.join('\n')
  }

  async waitForText(text: string, timeoutMs = WAIT_TIMEOUT_MS): Promise<void> {
    const deadline = performance.now() + timeoutMs
    while (!this.screenText().includes(text)) {
      if (performance.now() > deadline) throw new Error(`timed out waiting for "${text}" on:\n${this.screenText()}`)
      await sleep(WAIT_POLL_MS)
    }
  }

  async waitForNoText(text: string, timeoutMs = WAIT_TIMEOUT_MS): Promise<void> {
    const deadline = performance.now() + timeoutMs
    while (this.screenText().includes(text)) {
      if (performance.now() > deadline) throw new Error(`"${text}" stayed on screen:\n${this.screenText()}`)
      await sleep(WAIT_POLL_MS)
    }
  }

  send(keys: string): void {
    this.child.stdin!.write(keys)
  }

  // Types like a person: one character every `delayMs`.
  async type(text: string, delayMs: number): Promise<void> {
    for (const character of text) {
      this.send(character)
      await sleep(delayMs)
    }
  }

  pause(ms: number): Promise<void> {
    return sleep(ms)
  }

  // Everything the terminal has received so far; a "mark" for rendering a still of this moment.
  outputSoFar(): string {
    return this.events.map(([, , text]) => text).join('')
  }

  saveCast(file: string): void {
    const header = { version: 2, width: this.options.cols, height: this.options.rows }
    const lines = [header, ...this.events].map((entry) => JSON.stringify(entry))
    writeFileSync(file, `${lines.join('\n')}\n`)
  }

  // Quits `ref` the normal way, then ends the `script` process this object started.
  async close(): Promise<void> {
    if (this.child.exitCode === null) {
      this.send(QUIT_KEYS)
      await Promise.race([this.exited, sleep(QUIT_GRACE_MS)])
    }
    if (this.child.exitCode === null) {
      this.child.kill()
      await this.exited
    }
    this.screen.dispose()
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}
