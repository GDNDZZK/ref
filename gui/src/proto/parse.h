#pragma once

#include "proto/types.h"

#include <QJsonObject>
#include <QJsonValue>
#include <QString>

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

// Results of requests (the `result` of a ResponseFrame).
Parsed<HelloResult> parseHelloResult(const QJsonObject& result);               // hello
Parsed<SessionInfo> parseSessionResult(const QJsonObject& result);             // create_session, set_*
Parsed<SessionSnapshot> parseAttachResult(const QJsonObject& result);          // attach
Parsed<QList<SessionInfo>> parseSessionsResult(const QJsonObject& result);     // list_sessions, list_history, watch_all
Parsed<PromptResult> parsePromptResult(const QJsonObject& result);             // prompt
Parsed<MessagesPage> parseMessagesResult(const QJsonObject& result);           // messages

} // namespace refgui::proto
