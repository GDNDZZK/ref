#include "proto/parse.h"
#include "proto/parse_internal.h"

using namespace Qt::StringLiterals;

namespace refgui::proto
{
namespace json
{
namespace
{

Fields child(const Fields& fields, QLatin1StringView key)
{
	return Fields(fields.object(key), fields.childPath(key));
}

Delta readDelta(const Fields& fields)
{
	const QString type = fields.string("type"_L1);
	if(type == "text"_L1)
	{
		return {.kind = Delta::Kind::Text, .text = fields.string("text"_L1)};
	}
	if(type == "reasoning"_L1)
	{
		return {.kind = Delta::Kind::Reasoning, .text = fields.string("text"_L1)};
	}
	if(type == "tool_input"_L1)
	{
		return {.kind = Delta::Kind::ToolInput, .text = fields.string("text"_L1)};
	}
	failUnknown(fields.childPath("type"_L1), u"unknown delta type \"%1\""_s.arg(type));
}

SessionEvent readSessionEvent(const Fields& fields)
{
	const QString type = fields.string("type"_L1);
	if(type == "info"_L1)
	{
		return InfoEvent{.info = readSessionInfo(child(fields, "info"_L1))};
	}
	if(type == "message"_L1)
	{
		return MessageEvent{.message = readMessage(child(fields, "message"_L1))};
	}
	if(type == "part"_L1)
	{
		return PartEvent{.messageId = fields.string("message_id"_L1),
		                 .index = fields.integer("index"_L1),
		                 .part = readPart(child(fields, "part"_L1))};
	}
	if(type == "delta"_L1)
	{
		return DeltaEvent{.messageId = fields.string("message_id"_L1),
		                  .index = fields.integer("index"_L1),
		                  .delta = readDelta(child(fields, "delta"_L1))};
	}
	if(type == "removed"_L1)
	{
		return RemovedEvent{.messageId = fields.string("message_id"_L1)};
	}
	if(type == "reset"_L1)
	{
		return ResetEvent{.messages = readMessages(fields, "messages"_L1), .moreBefore = fields.booleanOr("more_before"_L1, false)};
	}
	if(type == "permission"_L1)
	{
		return PermissionEvent{.request = readPermissionRequest(child(fields, "request"_L1))};
	}
	if(type == "permission_resolved"_L1)
	{
		return PermissionResolvedEvent{.callId = fields.string("call_id"_L1)};
	}
	if(type == "queue"_L1)
	{
		return QueueEvent{.messages = readMessages(fields, "messages"_L1)};
	}
	if(type == "error"_L1)
	{
		return ErrorEvent{.text = fields.string("text"_L1)};
	}
	if(type == "closed"_L1)
	{
		return ClosedEvent{};
	}
	failUnknown(fields.childPath("type"_L1), u"unknown event type \"%1\""_s.arg(type));
}

ToastFrame readToast(const Fields& fields)
{
	const QString level = fields.string("level"_L1);
	if(level == "info"_L1)
	{
		return {.level = ToastFrame::Level::Info, .text = fields.string("text"_L1)};
	}
	if(level == "warn"_L1)
	{
		return {.level = ToastFrame::Level::Warn, .text = fields.string("text"_L1)};
	}
	if(level == "error"_L1)
	{
		return {.level = ToastFrame::Level::Error, .text = fields.string("text"_L1)};
	}
	failUnknown(fields.childPath("level"_L1), u"unknown toast level \"%1\""_s.arg(level));
}

NotifyFrame readNotify(const Fields& fields)
{
	const QString kind = fields.string("notify_kind"_L1);
	NotifyFrame notify{.sessionId = fields.string("session_id"_L1),
	                   .sessionTitle = fields.string("session_title"_L1),
	                   .detail = fields.optionalString("detail"_L1)};
	if(kind == "permission"_L1)
	{
		notify.kind = NotifyFrame::Kind::Permission;
	}
	else if(kind == "turn_done"_L1)
	{
		notify.kind = NotifyFrame::Kind::TurnDone;
	}
	else if(kind == "turn_error"_L1)
	{
		notify.kind = NotifyFrame::Kind::TurnError;
	}
	else
	{
		failUnknown(fields.childPath("notify_kind"_L1), u"unknown notification kind \"%1\""_s.arg(kind));
	}
	return notify;
}

QList<SessionInfo> readSessions(const Fields& fields)
{
	return fields.list<SessionInfo>("sessions"_L1, readSessionInfo);
}

ServerFrame readServerFrame(const Fields& fields)
{
	const QString kind = fields.string("kind"_L1);
	if(kind == "response"_L1)
	{
		ResponseFrame response{.id = static_cast<quint64>(fields.integer("id"_L1)), .ok = fields.booleanOr("ok"_L1, false)};
		response.result = fields.optionalObject("result"_L1).value_or(QJsonObject());
		response.error = fields.optionalString("error"_L1).value_or(QString());
		return response;
	}
	if(kind == "event"_L1)
	{
		return EventFrame{.sessionId = fields.string("session_id"_L1), .event = readSessionEvent(child(fields, "event"_L1))};
	}
	if(kind == "sessions"_L1)
	{
		return SessionsFrame{.sessions = readSessions(fields)};
	}
	if(kind == "toast"_L1)
	{
		return readToast(fields);
	}
	if(kind == "notify"_L1)
	{
		return readNotify(fields);
	}
	failUnknown(fields.childPath("kind"_L1), u"unknown frame kind \"%1\""_s.arg(kind));
}

SessionSnapshot readSnapshot(const Fields& fields)
{
	return {.info = readSessionInfo(child(fields, "info"_L1)),
	        .messages = readMessages(fields, "messages"_L1),
	        .moreBefore = fields.booleanOr("more_before"_L1, false),
	        .pendingPermissions = fields.listOrEmpty<PermissionRequest>("pending_permissions"_L1, readPermissionRequest),
	        .queued = fields.listOrEmpty<Message>("queued"_L1, readMessage)};
}

} // namespace
} // namespace json

Parsed<ServerFrame> parseServerFrame(const QJsonObject& frame)
{
	return json::guarded([&] { return json::readServerFrame(json::Fields(frame, u"frame"_s)); });
}

Parsed<SessionEvent> parseSessionEvent(const QJsonObject& event)
{
	return json::guarded([&] { return json::readSessionEvent(json::Fields(event, u"event"_s)); });
}

Parsed<Delta> parseDelta(const QJsonObject& delta)
{
	return json::guarded([&] { return json::readDelta(json::Fields(delta, u"delta"_s)); });
}

Parsed<HelloResult> parseHelloResult(const QJsonObject& result)
{
	return json::guarded([&] {
		const json::Fields fields(result, u"hello"_s);
		return HelloResult{.serverVersion = fields.string("server_version"_L1), .protocol = static_cast<int>(fields.integerOr("protocol"_L1, 1))};
	});
}

Parsed<SessionInfo> parseSessionResult(const QJsonObject& result)
{
	return json::guarded([&] { return json::readSessionInfo(json::child(json::Fields(result, u"result"_s), "session"_L1)); });
}

Parsed<SessionSnapshot> parseAttachResult(const QJsonObject& result)
{
	return json::guarded([&] { return json::readSnapshot(json::child(json::Fields(result, u"result"_s), "snapshot"_L1)); });
}

Parsed<QList<SessionInfo>> parseSessionsResult(const QJsonObject& result)
{
	return json::guarded([&] { return json::readSessions(json::Fields(result, u"result"_s)); });
}

Parsed<PromptResult> parsePromptResult(const QJsonObject& result)
{
	return json::guarded([&] {
		const json::Fields fields(result, u"result"_s);
		return PromptResult{.queued = fields.booleanOr("queued"_L1, false), .messageId = fields.string("message_id"_L1)};
	});
}

Parsed<MessagesPage> parseMessagesResult(const QJsonObject& result)
{
	return json::guarded([&] {
		const json::Fields fields(result, u"result"_s);
		return MessagesPage{.messages = json::readMessages(fields, "messages"_L1), .moreBefore = fields.booleanOr("more_before"_L1, false)};
	});
}

Parsed<RemovedPrompt> parseRemovedPromptResult(const QJsonObject& result)
{
	return json::guarded([&] {
		const json::Fields fields(result, u"result"_s);
		return RemovedPrompt{.removed = fields.booleanOr("removed"_L1, false),
		                     .text = fields.optionalString("text"_L1).value_or(QString()),
		                     .images = fields.listOrEmpty<NamedImage>("images"_L1, json::readNamedImage)};
	});
}

Parsed<RemovedPrompt> parseUndoResult(const QJsonObject& result)
{
	auto parsed = parseRemovedPromptResult(result);
	if(parsed)
	{
		parsed->removed = true; // Undo answers only for what it removed
	}
	return parsed;
}

Parsed<QStringList> parseFilesResult(const QJsonObject& result)
{
	return json::guarded([&] {
		const json::Fields fields(result, u"result"_s);
		QStringList files;
		for(const QJsonValue& file : fields.array("files"_L1))
		{
			if(!file.isString())
			{
				json::fail(fields.childPath("files"_L1), u"expected a list of strings"_s);
			}
			files.append(file.toString());
		}
		return files;
	});
}

Parsed<QList<NamedImage>> parseNamedImages(const QJsonValue& images)
{
	return json::guarded([&] { return json::Fields(QJsonObject{{u"images"_s, images}}, u"result"_s).listOrEmpty<NamedImage>("images"_L1, json::readNamedImage); });
}

Parsed<ImageData> parseImageResult(const QJsonObject& result)
{
	return json::guarded([&] { return json::readImageData(json::child(json::Fields(result, u"result"_s), "image"_L1)); });
}

} // namespace refgui::proto
