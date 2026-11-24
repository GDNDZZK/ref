// The protocol's requests as typed methods. The result shapes are the ones in
// crates/proto/fixtures/server_frame/response_*.json.

import type {
  ConfigFilesInfo,
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
  StashGroup,
} from '../proto/generated'
import type { RefConnection } from './connection.ts'

export type ShellCompletionItem = { value: string; description?: string }
export interface MessagesPage {
  messages: Message[]
  more_before: boolean
  start_seq: number
  end_seq: number
}
export class RefApi {
  private readonly connection: RefConnection

  constructor(connection: RefConnection) {
    this.connection = connection
  }

  async getConfig(cwd?: string): Promise<ServerConfigInfo> {
    return this.call({ op: 'get_config', ...(cwd !== undefined ? { cwd } : {}) }, 'config')
  }

  async reloadConfig(cwd?: string): Promise<{ config: ServerConfigInfo; warnings: string[] }> {
    return this.callWhole({ op: 'reload_config', ...(cwd !== undefined ? { cwd } : {}) })
  }

  async getConfigFiles(file?: string, cwd?: string): Promise<ConfigFilesInfo> {
    return this.callWhole({
      op: 'get_config_files',
      ...(file !== undefined ? { file } : {}),
      ...(cwd !== undefined ? { cwd } : {}),
    })
  }

  async saveConfigFile(file: string, content: string, cwd?: string): Promise<ConfigFilesInfo> {
    return this.callWhole({
      op: 'save_config_file',
      file,
      content,
      ...(cwd !== undefined ? { cwd } : {}),
    })
  }

  async createConfigFile(
    options: {
      label?: string
      cloneCurrent?: boolean
      content?: string
      cwd?: string
    } = {},
  ): Promise<ConfigFilesInfo> {
    return this.callWhole({
      op: 'create_config_file',
      ...(options.label !== undefined ? { label: options.label } : {}),
      clone_current: options.cloneCurrent ?? true,
      ...(options.content !== undefined ? { content: options.content } : {}),
      ...(options.cwd !== undefined ? { cwd: options.cwd } : {}),
    })
  }

  async selectConfigFile(file: string, cwd?: string): Promise<ConfigFilesInfo> {
    return this.callWhole({
      op: 'select_config_file',
      file,
      ...(cwd !== undefined ? { cwd } : {}),
    })
  }

  async deleteConfigFile(file: string, cwd?: string): Promise<ConfigFilesInfo> {
    return this.callWhole({
      op: 'delete_config_file',
      file,
      ...(cwd !== undefined ? { cwd } : {}),
    })
  }

  async listSessions(): Promise<SessionInfo[]> {
    return this.call({ op: 'list_sessions' }, 'sessions')
  }

  // Persisted and live sessions, newest first.
  async listHistory(
    options: { cwd?: string; creator?: string; query?: string; limit?: number } = {},
  ): Promise<SessionInfo[]> {
    return this.call(
      {
        op: 'list_history',
        cwd: options.cwd ?? null,
        creator: options.creator ?? null,
        query: options.query ?? null,
        limit: options.limit ?? null,
      },
      'sessions',
    )
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

  // Sub-directory (and optional file) names of `path`, sorted. `path` comes back absolute and normalized.
  async listDirs(
    path: string,
    options: { cwd?: string; includeFiles?: boolean } = {},
  ): Promise<{ path: string; dirs: string[]; files: string[] }> {
    return this.callWhole({
      op: 'list_dirs',
      path,
      ...(options.cwd !== undefined ? { cwd: options.cwd } : {}),
      include_files: options.includeFiles ?? false,
    })
  }

  async listBinaries(query: string, limit?: number): Promise<string[]> {
    return this.call({ op: 'list_binaries', query, ...(limit !== undefined ? { limit } : {}) }, 'binaries')
  }

  async completeShell(
    cwd: string,
    line: string,
    options?: { cursor?: number; limit?: number },
  ): Promise<ShellCompletionItem[]> {
    return this.call(
      {
        op: 'complete_shell',
        cwd,
        line,
        ...(options?.cursor !== undefined ? { cursor: options.cursor } : {}),
        ...(options?.limit !== undefined ? { limit: options.limit } : {}),
      },
      'items',
    )
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
  async messagesBefore(sessionId: string, before: string, limit?: number): Promise<MessagesPage> {
    return this.callWhole({ op: 'messages', session_id: sessionId, before, limit: limit ?? null })
  }

  // A contiguous range of messages [startSeq, endSeq), oldest first.
  async messagesRange(
    sessionId: string,
    startSeq: number,
    endSeq: number,
    limit?: number,
  ): Promise<MessagesPage> {
    return this.callWhole({
      op: 'messages',
      session_id: sessionId,
      start_seq: startSeq,
      end_seq: endSeq,
      limit: limit ?? null,
    })
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

  async retryNow(sessionId: string): Promise<boolean> {
    return this.call({ op: 'retry_now', session_id: sessionId }, 'retried')
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

  async listGroups(): Promise<StashGroup[]> {
    return this.call({ op: 'list_groups' }, 'groups')
  }

  async stashAll(name?: string): Promise<{ group: StashGroup | null; groups: StashGroup[] }> {
    return this.callWhole({ op: 'stash_all', name: name ?? null })
  }

  async createGroup(
    sessionIds: string[],
    options: { name?: string; stashLive?: boolean } = {},
  ): Promise<{ group: StashGroup; groups: StashGroup[] }> {
    return this.callWhole({
      op: 'create_group',
      name: options.name ?? null,
      session_ids: sessionIds,
      stash_live: options.stashLive ?? true,
    })
  }

  async renameGroup(groupId: string, name: string): Promise<StashGroup[]> {
    return this.call({ op: 'rename_group', group_id: groupId, name }, 'groups')
  }

  async setGroupLocked(groupId: string, locked: boolean): Promise<StashGroup[]> {
    return this.call({ op: 'set_group_locked', group_id: groupId, locked }, 'groups')
  }

  async deleteGroup(groupId: string): Promise<StashGroup[]> {
    return this.call({ op: 'delete_group', group_id: groupId }, 'groups')
  }

  async addToGroup(
    groupId: string,
    sessionId: string,
    options: { position?: number; stashLive?: boolean } = {},
  ): Promise<StashGroup[]> {
    return this.call(
      {
        op: 'add_to_group',
        group_id: groupId,
        session_id: sessionId,
        position: options.position ?? null,
        stash_live: options.stashLive ?? true,
      },
      'groups',
    )
  }

  async moveGroupMember(
    toGroupId: string,
    sessionId: string,
    position: number,
    options: { fromGroupId?: string; stashLive?: boolean } = {},
  ): Promise<StashGroup[]> {
    return this.call(
      {
        op: 'move_group_member',
        from_group_id: options.fromGroupId ?? null,
        to_group_id: toGroupId,
        session_id: sessionId,
        position,
        stash_live: options.stashLive ?? true,
      },
      'groups',
    )
  }

  async removeFromGroup(groupId: string, sessionId: string): Promise<StashGroup[]> {
    return this.call({ op: 'remove_from_group', group_id: groupId, session_id: sessionId }, 'groups')
  }

  async mergeGroups(sourceGroupId: string, targetGroupId: string): Promise<StashGroup[]> {
    return this.call(
      { op: 'merge_groups', source_group_id: sourceGroupId, target_group_id: targetGroupId },
      'groups',
    )
  }

  async restoreGroup(groupId: string): Promise<{ restored_ids: string[]; groups: StashGroup[] }> {
    return this.callWhole({ op: 'restore_group', group_id: groupId })
  }

  async restoreGroupMember(groupId: string, sessionId: string): Promise<{ restored: boolean; groups: StashGroup[] }> {
    return this.callWhole({ op: 'restore_group_member', group_id: groupId, session_id: sessionId })
  }
}
