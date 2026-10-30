// The protocol's requests as typed methods. The result shapes are the ones in
// crates/proto/fixtures/server_frame/response_*.json.

import type {
  ImageData,
  ModelRef,
  NamedImage,
  NotifyKind,
  ServerConfigInfo,
  SessionInfo,
  SessionSnapshot,
} from '../proto/generated'
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

  // `removed` is false when the message was delivered in the meantime.
  async removeQueued(
    sessionId: string,
    messageId: string,
  ): Promise<{ removed: boolean; text: string; images: NamedImage[] }> {
    const result = await this.connection.request({
      op: 'remove_queued',
      session_id: sessionId,
      message_id: messageId,
    })
    return result as unknown as { removed: boolean; text: string; images: NamedImage[] }
  }

  // The output is added to the conversation as events.
  async shell(sessionId: string, command: string): Promise<void> {
    await this.connection.request({ op: 'shell', session_id: sessionId, command })
  }

  // Relative paths; directories end with `/`.
  async listFiles(cwd: string, query: string, limit: number): Promise<string[]> {
    const result = (await this.connection.request({ op: 'list_files', cwd, query, limit })) as unknown as {
      files: string[]
    }
    return result.files
  }

  async expandCommand(name: string, args: string, cwd: string): Promise<string> {
    const result = (await this.connection.request({ op: 'expand_command', name, args, cwd })) as unknown as {
      text: string
    }
    return result.text
  }

  // Replaces the kinds this connection wants; the handshake carries the first choice.
  async setNotify(kinds: NotifyKind[]): Promise<void> {
    await this.connection.request({ op: 'set_notify', kinds })
  }

  // Whether the tab is in front and which sessions it shows, so the backend does not notify about those.
  async setFocus(focused: boolean, sessions: string[]): Promise<void> {
    await this.connection.request({ op: 'set_focus', focused, sessions })
  }
}
