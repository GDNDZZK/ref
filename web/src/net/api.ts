// The protocol's requests as typed methods. The result shapes are the ones in
// crates/proto/fixtures/server_frame/response_*.json.

import type { ImageData, ModelRef, ServerConfigInfo, SessionInfo, SessionSnapshot } from '../proto/generated'
import type { RefConnection } from './connection.ts'

export class RefApi {
  private readonly connection: RefConnection

  constructor(connection: RefConnection) {
    this.connection = connection
  }

  async getConfig(): Promise<ServerConfigInfo> {
    const result = (await this.connection.request({ op: 'get_config' })) as unknown as {
      config: ServerConfigInfo
    }
    return result.config
  }

  async listSessions(): Promise<SessionInfo[]> {
    const result = (await this.connection.request({ op: 'list_sessions' })) as unknown as {
      sessions: SessionInfo[]
    }
    return result.sessions
  }

  // Does not attach.
  async createSession(
    cwd: string,
    options: { agent?: string; model?: ModelRef; name?: string } = {},
  ): Promise<SessionInfo> {
    const result = (await this.connection.request({
      op: 'create_session',
      cwd,
      agent: options.agent ?? null,
      model: options.model ?? null,
      name: options.name ?? null,
    })) as unknown as { session: SessionInfo }
    return result.session
  }

  // Events for the session follow the returned snapshot.
  async attach(sessionId: string, foreground: boolean): Promise<SessionSnapshot> {
    const result = (await this.connection.request({
      op: 'attach',
      session_id: sessionId,
      foreground,
    })) as unknown as { snapshot: SessionSnapshot }
    return result.snapshot
  }

  async detach(sessionId: string): Promise<void> {
    await this.connection.request({ op: 'detach', session_id: sessionId })
  }

  // `queued`: the session was busy, the message waits for the next safe point.
  async prompt(
    sessionId: string,
    text: string,
    images: ImageData[] = [],
  ): Promise<{ queued: boolean; message_id: string }> {
    const result = await this.connection.request({ op: 'prompt', session_id: sessionId, text, images })
    return result as unknown as { queued: boolean; message_id: string }
  }

  async abort(sessionId: string): Promise<void> {
    await this.connection.request({ op: 'abort', session_id: sessionId })
  }

  async permissionReply(sessionId: string, callId: string, allow: boolean, always: boolean): Promise<void> {
    await this.connection.request({
      op: 'permission_reply',
      session_id: sessionId,
      call_id: callId,
      allow,
      always,
    })
  }
}
