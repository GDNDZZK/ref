// @vitest-environment node
/// <reference types="node" />
// Runs every protocol sample from crates/proto/fixtures through the code that reads it.

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { RefConnection } from '../net/connection.ts'
import type { TransportHandlers } from '../net/transport.ts'
import { Transcript } from '../model/transcript.ts'
import type { ClientFrame, ServerFrame, SessionEvent, SessionSnapshot } from './generated'

const FIXTURES = fileURLToPath(new URL('../../../crates/proto/fixtures', import.meta.url))
const LATER = 100_000

function fixtures<T>(directory: string): [string, T][] {
  const path = join(FIXTURES, directory)
  return readdirSync(path)
    .filter((name) => name.endsWith('.json'))
    .sort()
    .map((name) => [name, JSON.parse(readFileSync(join(path, name), 'utf8')) as T])
}

// Every variant must have a sample; adding a variant to the protocol makes these fail to compile.
const SESSION_EVENT_TYPES = {
  info: 0,
  message: 0,
  part: 0,
  delta: 0,
  removed: 0,
  reset: 0,
  permission: 0,
  permission_resolved: 0,
  queue: 0,
  error: 0,
  closed: 0,
} satisfies Record<SessionEvent['type'], 0>

const SERVER_FRAME_KINDS = { response: 0, event: 0, sessions: 0, toast: 0, notify: 0 } satisfies Record<
  ServerFrame['kind'],
  0
>

const REQUEST_OPS = [
  'hello',
  'get_config',
  'list_sessions',
  'list_history',
  'create_session',
  'attach',
  'messages',
  'get_image',
  'detach',
  'watch_all',
  'prompt',
  'remove_queued',
  'shell',
  'abort',
  'permission_reply',
  'set_model',
  'set_agent',
  'rename',
  'set_cwd',
  'compact',
  'undo',
  'close',
  'delete',
  'expand_command',
  'list_files',
  'set_variant',
  'export',
  'peer_command',
  'mcp_status',
  'set_notify',
  'set_focus',
  'list_dirs',
  'shutdown',
  'ping',
] satisfies ClientFrame['op'][]

describe('request samples', () => {
  const samples = fixtures<ClientFrame>('request')

  it('cover every op', () => {
    expect(new Set(samples.map(([, frame]) => frame.op))).toEqual(new Set(REQUEST_OPS))
  })
})

describe('server frame samples', () => {
  const samples = fixtures<ServerFrame>('server_frame')

  it('cover every kind', () => {
    expect(new Set(samples.map(([, frame]) => frame.kind))).toEqual(new Set(Object.keys(SERVER_FRAME_KINDS)))
  })

  it.each(samples)('%s is read by the connection without complaint', (_name, frame) => {
    let handlers: TransportHandlers | undefined
    const connection = new RefConnection({
      connect: (h) => {
        handlers = h
        return { send: () => {}, close: () => {} }
      },
      client: 'test',
      cwd: '/',
    })
    const errors: Error[] = []
    const frames: string[] = []
    connection.onError((error) => errors.push(error))
    connection.onFrame((f) => frames.push(f.kind))
    connection.open()

    handlers!.message(JSON.stringify(frame))

    // A response nobody asked for is the one thing reported; every other frame is delivered.
    if (frame.kind === 'response')
      expect(errors.map((e) => e.message)).toEqual([`response for unknown request ${frame.id}`])
    else expect({ errors: errors.length, frames }).toEqual({ errors: 0, frames: [frame.kind] })
  })
})

describe('session event samples', () => {
  const samples = fixtures<SessionEvent>('session_event')
  const attach = JSON.parse(readFileSync(join(FIXTURES, 'server_frame', 'response_attach.json'), 'utf8')) as {
    result: { snapshot: SessionSnapshot }
  }

  it('cover every type', () => {
    expect(new Set(samples.map(([, event]) => event.type))).toEqual(new Set(Object.keys(SESSION_EVENT_TYPES)))
  })

  it('the attach sample loads into blocks', () => {
    const transcript = new Transcript({ reportError: (message) => expect.fail(message) })
    transcript.load(attach.result.snapshot)
    transcript.flush(0)
    expect(transcript.blocks.length).toBeGreaterThan(0)
  })

  it.each(samples)('%s applies to a loaded transcript without complaint', (_name, event) => {
    const errors: string[] = []
    const transcript = new Transcript({ reportError: (message) => errors.push(message) })
    transcript.load(attach.result.snapshot)
    transcript.flush(0)

    transcript.apply(event, 0)
    transcript.flush(LATER)

    expect(errors).toEqual([])
  })
})
