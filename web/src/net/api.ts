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
    return this.call({ op: 'get_config' }, 'config')
  }

  async listSessions(): Promise<SessionInfo[]> {
    return this.call({ op: 'list_sessions' }, 'sessions')
  }

  // Persisted and live sessions, newest first.
  async listHistory(cwd?: string, limit?: number): Promise<SessionInfo[]> {
    return this.call({ op: 'list_history', cwd: cwd ?? null, limit: limit ?? null }, 'sessions')
  }

  // Events of every live session follow until `enabled: false`. Returns the live sessions.
  async watchAll(enabled: boolean): Promise<SessionInfo[]> {
    return this.call({ op: 'watch_all', enabled }, 'sessions')
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
    return this.call({ op: 'export', session_id: sessionId, path }, 'path')
  }

  // `command`: "peers" | "peers-name" | "peers-inbox" | "peers-outbox". Returns text to show.
  async peerCommand(sessionId: string, command: string, args = ''): Promise<string> {
    return this.call({ op: 'peer_command', session_id: sessionId, command, args }, 'text')
  }

  async mcpStatus(cwd?: string): Promise<McpServerStatus[]> {
    return this.call({ op: 'mcp_status', cwd: cwd ?? null }, 'servers')
  }

  // Sub-directory names of `path`, sorted. `path` comes back absolute and normalized.
  async listDirs(path: string): Promise<{ path: string; dirs: string[] }> {
    return this.callWhole({ op: 'list_dirs', path })
  }

  async shutdown(): Promise<void> {
    await this.connection.request({ op: 'shutdown' })
  }

  private async sessionResult(request: Request): Promise<SessionInfo> {
    return this.call(request, 'session')
  }

  // The result is an object; `key` picks the field the caller wants.
  private async call<T>(request: Request, key: string): Promise<T> {
    const result = await this.callWhole<Record<string, T>>(request)
    return result[key]!
  }

  private async callWhole<T>(request: Request): Promise<T> {
    return (await this.connection.request(request)) as unknown as T
  }

  // Does not attach.
  async createSession(
    cwd: string,
    options: { agent?: string; model?: ModelRef; name?: string } = {},
  ): Promise<SessionInfo> {
    return this.call(
      {
        op: 'create_session',
        cwd,
        agent: options.agent ?? null,
        model: options.model ?? null,
        name: options.name ?? null,
      },
      'session',
    )
  }

  // Events for the session follow the returned snapshot.
  async attach(sessionId: string, foreground: boolean): Promise<SessionSnapshot> {
    return this.call({ op: 'attach', session_id: sessionId, foreground }, 'snapshot')
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
    return this.callWhole({ op: 'messages', session_id: sessionId, before, limit: limit ?? null })
  }

  // The bytes of an image the messages only carry as a stub.
  async getImage(sessionId: string, imageId: string): Promise<{ mime: string; data: string }> {
    return this.call({ op: 'get_image', session_id: sessionId, image_id: imageId }, 'image')
  }

  // Removes the message and all after it (the last human prompt without `messageId`).
  async undo(sessionId: string, messageId?: string): Promise<{ text: string; images: NamedImage[] }> {
    return this.callWhole({ op: 'undo', session_id: sessionId, message_id: messageId ?? null })
  }

  // `queued`: the session was busy, the message waits for the next safe point.
  async prompt(
    sessionId: string,
    text: string,
    images: ImageData[] = [],
  ): Promise<{ queued: boolean; message_id: string }> {
    return this.callWhole({ op: 'prompt', session_id: sessionId, text, images })
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
    return this.callWhole({ op: 'remove_queued', session_id: sessionId, message_id: messageId })
  }

  // The output is added to the conversation as events.
  async shell(sessionId: string, command: string): Promise<void> {
    await this.connection.request({ op: 'shell', session_id: sessionId, command })
  }

  // Relative paths; directories end with `/`.
  async listFiles(cwd: string, query: string, limit: number): Promise<string[]> {
    return this.call({ op: 'list_files', cwd, query, limit }, 'files')
  }

  async expandCommand(name: string, args: string, cwd: string): Promise<string> {
    return this.call({ op: 'expand_command', name, args, cwd }, 'text')
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
