// Casts against a real `ref-server` (no mock), in a shell with a fixture home: no key of the
// user's, private state, and no prompt is sent, so no model request is made.
//   fixture-start  RB1: `cd ~/project`, `ref` starts on the backend started here, then the model
//                  dialog (ctrl+x m) and the agent dialog (ctrl+x a). Film C7, Film A A11 f3 and A12.
//   server-start   RB2: `ref-server` typed in the shell, its startup line. Film A A10, C8.
import { spawn, type ChildProcess } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { OPENCODE_JSON } from '../src/films/opencodeConfig.ts'
import { isolatedEnv, type IsolatedEnv } from './lib/backend.ts'
import { binary } from './lib/binaries.ts'
import { KEYS, TuiSession } from './lib/tuiSession.ts'
import { TUI_COLS, TUI_ROWS } from './tui.ts'

const CASTS_DIR = join(import.meta.dirname, '..', 'assets', 'casts')
const PROJECT = 'project'
const SHELL_RC = '.promo-bashrc' // sets the prompt; bash ignores PS1 from the environment
const PROMPT = '$ '
const TYPING_DELAY_MS = 70
const READY_TEXT = 'Build ·' // the composer's status line
const START_TIMEOUT_MS = 30_000
const LISTENING_TEXT = 'ref-server listening on'
const DIALOG_SHOWN_MS = 3000
const BEAT_MS = 800
// Keys the dialogs need to list the providers' models. Made up; nothing is sent with them.
const DUMMY_KEYS = { ANTHROPIC_API_KEY: 'sk-ant-promo', DEEPSEEK_API_KEY: 'sk-promo', WORK_API_KEY: 'promo' }
const MODEL_DIALOG_KEYS = `${KEYS.ctrl('x')}m`
const AGENT_DIALOG_KEYS = `${KEYS.ctrl('x')}a`
const INTERRUPT = '\x03' // ctrl+c

// A home with only the opencode.json of the film and a small project.
function fixtureHome(isolated: IsolatedEnv): { home: string; env: NodeJS.ProcessEnv } {
  const home = join(isolated.root, 'home')
  const config = join(home, '.config', 'opencode', 'opencode.json')
  mkdirSync(dirname(config), { recursive: true })
  writeFileSync(config, `${OPENCODE_JSON.join('\n')}\n`)
  mkdirSync(join(home, PROJECT, 'src'), { recursive: true })
  writeFileSync(join(home, PROJECT, 'src', 'parser.rs'), 'pub fn parse_row(text: &str) -> Vec<&str> {\n    text.split(\',\').collect()\n}\n')
  writeFileSync(join(home, PROJECT, 'README.md'), '# project\n')
  writeFileSync(join(home, SHELL_RC), `PS1='${PROMPT}'\n`)

  // The user's own keys stay out of the take.
  const inherited = Object.fromEntries(Object.entries(isolated.env).filter(([name]) => !name.endsWith('_API_KEY')))
  const env: NodeJS.ProcessEnv = {
    ...inherited,
    ...DUMMY_KEYS,
    HOME: home,
    XDG_CONFIG_HOME: join(home, '.config'),
    PATH: `${dirname(binary('ref'))}:${process.env.PATH}`,
    HISTFILE: '/dev/null',
  }
  return { home, env }
}

// `ref-server` with the fixture home; `ref` finds it through the instance file in REF_STATE_DIR.
async function startRealServer(home: string, env: NodeJS.ProcessEnv): Promise<ChildProcess> {
  const child = spawn(binary('ref-server'), ['--idle-timeout', '-1'], { cwd: home, env, stdio: ['ignore', 'ignore', 'pipe'] })
  await new Promise<void>((resolve, reject) => {
    child.stderr!.on('data', (chunk: Buffer) => {
      if (chunk.toString().includes(LISTENING_TEXT)) resolve()
    })
    child.once('exit', (code) => reject(new Error(`ref-server exited with ${code}`)))
  })
  return child
}

async function stopServer(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null) return
  const exited = new Promise((resolve) => child.once('exit', resolve))
  child.kill('SIGTERM') // by the PID started above
  await exited
}

async function withShell(body: (shell: TuiSession, home: string, env: NodeJS.ProcessEnv) => Promise<void>): Promise<void> {
  const isolated = isolatedEnv()
  const { home, env } = fixtureHome(isolated)
  try {
    await body(new TuiSession({ cols: TUI_COLS, rows: TUI_ROWS, command: `bash --noprofile --rcfile ${join(home, SHELL_RC)} -i`, cwd: home }, undefined, { ...isolated, env }), home, env)
  } finally {
    isolated.remove()
  }
}

async function typeLine(shell: TuiSession, line: string, mark: string): Promise<void> {
  shell.mark(mark)
  await shell.type(line, TYPING_DELAY_MS)
  await shell.pause(BEAT_MS / 2)
  shell.send(KEYS.enter)
}

async function captureFixtureStart(): Promise<void> {
  await withShell(async (shell, home, env) => {
    const server = await startRealServer(home, env)
    try {
      await shell.waitForText(PROMPT.trimEnd()) // screen rows lose their trailing spaces
      await shell.pause(BEAT_MS)
      await typeLine(shell, `cd ~/${PROJECT}`, 'cd')
      await shell.pause(BEAT_MS)
      await typeLine(shell, 'ref', 'ref')
      await shell.waitForText(READY_TEXT, START_TIMEOUT_MS)
      shell.mark('ready')
      await shell.pause(2 * BEAT_MS)

      shell.mark('models')
      shell.send(MODEL_DIALOG_KEYS)
      await shell.pause(DIALOG_SHOWN_MS)
      console.log(`model dialog:\n${shell.screenText()}`)
      shell.send(KEYS.escape)
      await shell.pause(BEAT_MS)

      shell.mark('agents')
      shell.send(AGENT_DIALOG_KEYS)
      await shell.pause(DIALOG_SHOWN_MS)
      console.log(`agent dialog:\n${shell.screenText()}`)
      shell.send(KEYS.escape)
      await shell.pause(BEAT_MS)

      mkdirSync(CASTS_DIR, { recursive: true })
      shell.saveCast(join(CASTS_DIR, 'tui-fixture-start.cast'))
    } finally {
      await shell.close()
      await stopServer(server)
    }
  })
}

async function captureServerStart(): Promise<void> {
  await withShell(async (shell) => {
    try {
      await shell.waitForText(PROMPT.trimEnd()) // screen rows lose their trailing spaces
      await shell.pause(BEAT_MS)
      await typeLine(shell, 'ref-server', 'typed')
      await shell.waitForText(LISTENING_TEXT, START_TIMEOUT_MS)
      shell.mark('listening')
      await shell.pause(3 * BEAT_MS)
      mkdirSync(CASTS_DIR, { recursive: true })
      shell.saveCast(join(CASTS_DIR, 'tui-server-start.cast'))
      shell.send(INTERRUPT) // ends the ref-server the shell started
      await shell.pause(BEAT_MS)
    } finally {
      await shell.close()
    }
  })
}

export const REAL_CAPTURES: Record<string, () => Promise<void>> = {
  'fixture-start': captureFixtureStart,
  'server-start': captureServerStart,
}
