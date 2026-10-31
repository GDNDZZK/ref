// The protocol's requests as typed methods. The result shapes are the ones in
// crates/proto/fixtures/server_frame/response_*.json.

import type {
  ImageData,
  McpServerStatus,
  Message,
  ModelRef,
  NamedImage,
  NotifyKind,
  Request,
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

  // Persisted and live sessions, newest first.
  async listHistory(cwd?: string, limit?: number): Promise<SessionInfo[]> {
    const result = (await this.connection.request({
      op: 'list_history',
      cwd: cwd ?? null,
      limit: limit ?? null,
    })) as unknown as { sessions: SessionInfo[] }
    return result.sessions
  }

  // Events of every live session follow until `enabled: false`. Returns the live sessions.
  async watchAll(enabled: boolean): Promise<SessionInfo[]> {
    const result = (await this.connection.request({ op: 'watch_all', enabled })) as unknown as {
      sessions: SessionInfo[]
    }
    return result.sessions
  }

  async rename(sessionId: string, name: string): Promise<SessionInfo> {
    return this.sessionResult({ op: 'rename', session_id: sessionId, name })
  }

  // Unloads a live session; it stays in history.
  async close(sessionId: string): Promise<void> {
    await this.connection.request({ op: 'close', session_id: sessionId })
  }

  // Removes a session from history for good.
  async deleteSession(sessionId: string): Promise<void> {
    await this.connection.request({ op: 'delete', session_id: sessionId })
  }

  async setModel(sessionId: string, model: ModelRef): Promise<SessionInfo> {
    return this.sessionResult({ op: 'set_model', session_id: sessionId, model })
  }

  async setAgent(sessionId: string, agent: string): Promise<SessionInfo> {
    return this.sessionResult({ op: 'set_agent', session_id: sessionId, agent })
  }

  // Without `variant`, rotates to the next one. An empty string clears it.
  async setVariant(sessionId: string, variant?: string): Promise<SessionInfo> {
    return this.sessionResult({ op: 'set_variant', session_id: sessionId, variant: variant ?? null })
  }

  async setCwd(sessionId: string, path: string): Promise<SessionInfo> {
    return this.sessionResult({ op: 'set_cwd', session_id: sessionId, path })
  }

  async compact(sessionId: string): Promise<void> {
    await this.connection.request({ op: 'compact', session_id: sessionId })
  }

  // Writes the transcript on the backend's machine. Returns the absolute path written.
  async exportTranscript(sessionId: string, path: string): Promise<string> {
    const result = (await this.connection.request({
      op: 'export',
      session_id: sessionId,
      path,
    })) as unknown as {
      path: string
    }
    return result.path
  }

  // `command`: "peers" | "peers-name" | "peers-inbox" | "peers-outbox". Returns text to show.
  async peerCommand(sessionId: string, command: string, args = ''): Promise<string> {
    const result = (await this.connection.request({
      op: 'peer_command',
      session_id: sessionId,
      command,
      args,
    })) as unknown as { text: string }
    return result.text
  }

  async mcpStatus(cwd?: string): Promise<McpServerStatus[]> {
    const result = (await this.connection.request({ op: 'mcp_status', cwd: cwd ?? null })) as unknown as {
      servers: McpServerStatus[]
    }
    return result.servers
  }

  // Sub-directory names of `path`, sorted. `path` comes back absolute and normalized.
  async listDirs(path: string): Promise<{ path: string; dirs: string[] }> {
    const result = await this.connection.request({ op: 'list_dirs', path })
    return result as unknown as { path: string; dirs: string[] }
  }

  async shutdown(): Promise<void> {
    await this.connection.request({ op: 'shutdown' })
  }

  private async sessionResult(request: Request): Promise<SessionInfo> {
    const result = (await this.connection.request(request)) as unknown as { session: SessionInfo }
    return result.session
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

  // One page of messages older than `before`, oldest first.
  async messagesBefore(
    sessionId: string,
    before: string,
    limit?: number,
  ): Promise<{ messages: Message[]; more_before: boolean }> {
    const result = await this.connection.request({
      op: 'messages',
      session_id: sessionId,
      before,
      limit: limit ?? null,
    })
    return result as unknown as { messages: Message[]; more_before: boolean }
  }

  // The bytes of an image the messages only carry as a stub.
  async getImage(sessionId: string, imageId: string): Promise<{ mime: string; data: string }> {
    const result = (await this.connection.request({
      op: 'get_image',
      session_id: sessionId,
      image_id: imageId,
    })) as unknown as { image: { mime: string; data: string } }
    return result.image
  }

  // Removes the message and all after it (the last human prompt without `messageId`).
  async undo(sessionId: string, messageId?: string): Promise<{ text: string; images: NamedImage[] }> {
    const result = await this.connection.request({
      op: 'undo',
      session_id: sessionId,
      message_id: messageId ?? null,
    })
    return result as unknown as { text: string; images: NamedImage[] }
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

  // `answers` has one entry per question; null dismisses the questions.
  async questionReply(sessionId: string, callId: string, answers: string[][] | null): Promise<void> {
    await this.connection.request({ op: 'question_reply', session_id: sessionId, call_id: callId, answers })
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
