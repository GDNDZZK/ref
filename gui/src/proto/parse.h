#pragma once

#include "proto/types.h"

#include <QJsonObject>
#include <QJsonValue>
#include <QString>
#include <QStringList>

#include <expected>

namespace refgui::proto
{

struct ParseError
{
	QString message; // names the path of the offending value, e.g. "event.part.state: unknown tool state "x""
	// A frame kind, event type or enum value this client does not know, as opposed to a
	// malformed one. A newer backend may send them, so the caller logs and skips them.
	bool unknown = false;
};

template<typename T>
using Parsed = std::expected<T, ParseError>;

Parsed<ServerFrame> parseServerFrame(const QJsonObject& frame);
Parsed<SessionEvent> parseSessionEvent(const QJsonObject& event);
Parsed<Part> parsePart(const QJsonObject& part);
Parsed<Message> parseMessage(const QJsonObject& message);
Parsed<Delta> parseDelta(const QJsonObject& delta);
Parsed<ToolState> parseToolState(const QJsonValue& state);
Parsed<SessionInfo> parseSessionInfo(const QJsonObject& info);
Parsed<InstanceInfo> parseInstanceInfo(const QJsonObject& instance);

// Results of requests (the `result` of a ResponseFrame).
Parsed<HelloResult> parseHelloResult(const QJsonObject& result);               // hello
Parsed<SessionInfo> parseSessionResult(const QJsonObject& result);             // create_session, set_*
Parsed<SessionSnapshot> parseAttachResult(const QJsonObject& result);          // attach
Parsed<QList<SessionInfo>> parseSessionsResult(const QJsonObject& result);     // list_sessions, list_history, watch_all
Parsed<PromptResult> parsePromptResult(const QJsonObject& result);             // prompt
Parsed<MessagesPage> parseMessagesResult(const QJsonObject& result);           // messages
Parsed<ImageData> parseImageResult(const QJsonObject& result);                 // get_image
Parsed<ServerConfig> parseConfigResult(const QJsonObject& result);             // get_config
Parsed<ReloadConfigResult> parseReloadConfigResult(const QJsonObject& result); // reload_config
Parsed<ConfigFilesInfo> parseConfigFilesResult(const QJsonObject& result);     // *_config_file(s)
Parsed<QList<McpServerStatus>> parseMcpResult(const QJsonObject& result);      // mcp_status
Parsed<DirListing> parseDirsResult(const QJsonObject& result);                 // list_dirs
Parsed<QStringList> parseBinariesResult(const QJsonObject& result);            // list_binaries
Parsed<QList<ShellCompletionItem>> parseShellCompletionResult(const QJsonObject& result); // complete_shell
Parsed<QString> parseTextResult(const QJsonObject& result);                    // peer_command, expand_command
Parsed<QString> parsePathResult(const QJsonObject& result);                    // export
Parsed<RemovedPrompt> parseRemovedPromptResult(const QJsonObject& result);     // remove_queued
Parsed<RemovedPrompt> parseUndoResult(const QJsonObject& result);              // undo; `removed` is always true
Parsed<QStringList> parseFilesResult(const QJsonObject& result);               // list_files
Parsed<QList<NamedImage>> parseNamedImages(const QJsonValue& images);          // `images` of remove_queued and undo
Parsed<RetryNowResult> parseRetryNowResult(const QJsonObject& result);         // retry_now
Parsed<QList<StashGroup>> parseGroupsResult(const QJsonObject& result);        // list_groups, rename/lock/delete/add/move/remove/merge
Parsed<StashGroupResult> parseStashAllResult(const QJsonObject& result);       // stash_all
Parsed<StashGroupResult> parseCreateGroupResult(const QJsonObject& result);    // create_group
Parsed<RestoreGroupResult> parseRestoreGroupResult(const QJsonObject& result); // restore_group
Parsed<RestoreGroupMemberResult> parseRestoreGroupMemberResult(const QJsonObject& result); // restore_group_member

} // namespace refgui::proto
