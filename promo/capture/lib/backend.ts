import { spawn, type ChildProcess } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { createServer } from 'node:net'
import { join } from 'node:path'
import { binary } from './binaries.ts'
import { requirePrivateBus } from './privateBus.ts'

// Scratch state of the processes started here. Under promo/.cache, not /tmp, which is shared.
const RUN_DIR = join(import.meta.dirname, '..', '..', '.cache', 'run')
const STOP_GRACE_MS = 5000
const POLL_ATTEMPTS = 100
const POLL_INTERVAL_MS = 100

// Every process started here gets private state and data directories, so nothing can
// reach the user's real backend.
export interface IsolatedEnv {
  root: string
  env: NodeJS.ProcessEnv
  remove(): void
}

export function isolatedEnv(): IsolatedEnv {
  requirePrivateBus()
  mkdirSync(RUN_DIR, { recursive: true })
  const root = mkdtempSync(join(RUN_DIR, 'run-'))
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    REF_STATE_DIR: join(root, 'state'),
    REF_DATA_DIR: join(root, 'data'),
    TERM: 'xterm-256color',
    COLORTERM: 'truecolor',
  }
  delete env.NO_COLOR // the TUI turns colours off when it is set
  return { root, env, remove: () => rmSync(root, { recursive: true, force: true }) }
}

export async function freePort(): Promise<number> {
  const probe = createServer()
  await new Promise<void>((resolve) => probe.listen(0, '127.0.0.1', resolve))
  const address = probe.address()
  await new Promise((resolve) => probe.close(resolve))
  if (typeof address !== 'object' || address === null) throw new Error('no port')
  return address.port
}

async function waitFor<T>(what: string, attempt: () => T | undefined): Promise<T> {
  for (let tries = 0; tries < POLL_ATTEMPTS; tries++) {
    const value = attempt()
    if (value !== undefined) return value
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS))
  }
  throw new Error(`timed out waiting for ${what}`)
}

export interface Backend {
  // ws://host:port and the token, for `ref --server URL --token T`.
  url: string
  token: string
  stop(): Promise<void>
}

// `ref-server --mock`: a scripted fake backend that prints {"port":N,"token":"..."}.
// It exits when its stdin closes, which is how stop() ends it.
export async function startMockBackend(isolated: IsolatedEnv): Promise<Backend> {
  const child = spawn(binary('ref-server'), ['--mock'], {
    env: isolated.env,
    stdio: ['pipe', 'pipe', 'inherit'],
  })
  const firstLine = await new Promise<string>((resolve, reject) => {
    child.stdout!.once('data', (chunk: Buffer) => resolve(chunk.toString().split('\n')[0]!))
    child.once('error', reject)
  })
  const { port, token } = JSON.parse(firstLine) as { port: number; token: string }
  return { url: `ws://127.0.0.1:${port}/`, token, stop: () => endProcess(child) }
}

export interface WebServer {
  // Login address; opening it sets the cookie and redirects to the page.
  loginUrl: string
  stop(): Promise<void>
}

// `ref-web --mock` on a free port, or with `backend` a ref-web in front of that backend.
export async function startMockWeb(isolated: IsolatedEnv, backend?: Backend): Promise<WebServer> {
  const port = await freePort()
  const backendArgs = backend ? ['--server', backend.url, '--token', backend.token] : ['--mock']
  const child = spawn(binary('ref-web'), [...backendArgs, '--no-open', '--port', String(port)], {
    env: isolated.env,
    stdio: ['pipe', 'ignore', 'inherit'], // open stdin keeps the mock backend alive; stdout has the login token
  })
  const tokenFile = join(isolated.root, 'state', 'web', 'login-token')
  const token = await waitFor('the ref-web login token', () => {
    try {
      return readFileSync(tokenFile, 'utf8').trim() || undefined
    } catch {
      return undefined
    }
  })
  return { loginUrl: `http://127.0.0.1:${port}/login?token=${token}`, stop: () => endProcess(child) }
}

// Ends a process this module started: closes its stdin, asks it to stop and, if it
// lingers (open connections keep a server alive), kills it. Always by its own PID.
async function endProcess(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null) return
  const exited = new Promise((resolve) => child.once('exit', resolve))
  child.stdin?.end()
  child.kill('SIGTERM')
  const timer = setTimeout(() => child.kill('SIGKILL'), STOP_GRACE_MS)
  await exited
  clearTimeout(timer)
}
