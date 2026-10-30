#include "proto/parse.h"
#include "proto/parse_internal.h"

#include <array>
#include <utility>

using namespace Qt::StringLiterals;

namespace refgui::proto
{

bool isFinal(ToolState state)
{
	switch(state)
	{
	case ToolState::Done:
	case ToolState::Error:
	case ToolState::Denied:
	case ToolState::Cancelled:
		return true;
	case ToolState::Streaming:
	case ToolState::AwaitingPermission:
	case ToolState::Delayed:
	case ToolState::Running:
		return false;
	}
	Q_UNREACHABLE_RETURN(false);
}

namespace json
{
namespace
{

// Maps the wire name of an enum value to the value; unknown names fail as "unknown".
template<typename Enum, std::size_t N>
Enum readEnum(const QString& path, const QString& name, const std::array<std::pair<QLatin1StringView, Enum>, N>& names)
{
	for(const auto& [wireName, value] : names)
	{
		if(name == wireName)
		{
			return value;
		}
	}
	failUnknown(path, u"unknown value \"%1\""_s.arg(name));
}

TodoStatus readTodoStatus(const Fields& fields)
{
	static constexpr std::array<std::pair<QLatin1StringView, TodoStatus>, 4> names{{
		{"pending"_L1, TodoStatus::Pending},
		{"in_progress"_L1, TodoStatus::InProgress},
		{"completed"_L1, TodoStatus::Completed},
		{"cancelled"_L1, TodoStatus::Cancelled},
	}};
	return readEnum(fields.childPath("status"_L1), fields.string("status"_L1), names);
}

SessionStatus readSessionStatus(const Fields& fields)
{
	static constexpr std::array<std::pair<QLatin1StringView, SessionStatus::State>, 6> names{{
		{"idle"_L1, SessionStatus::State::Idle},
		{"busy"_L1, SessionStatus::State::Busy},
		{"running"_L1, SessionStatus::State::Running},
		{"waiting_permission"_L1, SessionStatus::State::WaitingPermission},
		{"retrying"_L1, SessionStatus::State::Retrying},
		{"compacting"_L1, SessionStatus::State::Compacting},
	}};
	SessionStatus status;
	status.state = readEnum(fields.childPath("state"_L1), fields.string("state"_L1), names);
	if(status.state == SessionStatus::State::Retrying)
	{
		status.attempt = static_cast<quint32>(fields.integer("attempt"_L1));
		status.nextAt = fields.integer("next_at"_L1);
		status.error = fields.string("error"_L1);
	}
	return status;
}

ContextUsage readContextUsage(const Fields& fields)
{
	return {.used = static_cast<quint64>(fields.integerOr("used"_L1, 0)),
	        .max = static_cast<quint64>(fields.integerOr("max"_L1, 0)),
	        .recommend = static_cast<quint64>(fields.integerOr("recommend"_L1, 0)),
	        .autoCompact = static_cast<quint64>(fields.integerOr("auto"_L1, 0))};
}

Usage readUsage(const Fields& fields)
{
	return {.input = static_cast<quint64>(fields.integer("input"_L1)),
	        .output = static_cast<quint64>(fields.integer("output"_L1)),
	        .cacheRead = static_cast<quint64>(fields.integer("cache_read"_L1)),
	        .cacheWrite = static_cast<quint64>(fields.integer("cache_write"_L1)),
	        .reasoning = static_cast<quint64>(fields.integerOr("reasoning"_L1, 0))};
}

MessageSource readMessageSource(const Fields& fields)
{
	static constexpr std::array<std::pair<QLatin1StringView, MessageSource::Kind>, 6> names{{
		{"human"_L1, MessageSource::Kind::Human},
		{"peer"_L1, MessageSource::Kind::Peer},
		{"spawn"_L1, MessageSource::Kind::Spawn},
		{"reminder"_L1, MessageSource::Kind::Reminder},
		{"compaction"_L1, MessageSource::Kind::Compaction},
		{"shell"_L1, MessageSource::Kind::Shell},
	}};
	MessageSource source;
	source.kind = readEnum(fields.childPath("kind"_L1), fields.string("kind"_L1), names);
	const bool hasSender = source.kind == MessageSource::Kind::Peer || source.kind == MessageSource::Kind::Spawn;
	if(hasSender)
	{
		source.fromId = fields.string("from_id"_L1);
		source.fromName = fields.string("from_name"_L1);
	}
	if(source.kind == MessageSource::Kind::Peer)
	{
		source.msgId = fields.string("msg_id"_L1);
	}
	return source;
}

ToolPart readToolPart(const Fields& fields)
{
	ToolPart tool;
	tool.callId = fields.string("call_id"_L1);
	tool.name = fields.string("name"_L1);
	tool.purpose = fields.optionalString("purpose"_L1).value_or(QString());
	tool.input = fields.value("input"_L1);
	tool.rawInput = fields.optionalString("raw_input"_L1).value_or(QString());
	tool.state = readToolState(fields.childPath("state"_L1), fields.value("state"_L1));
	tool.output = fields.optionalString("output"_L1);
	tool.meta = fields.value("meta"_L1);
	tool.images = fields.listOrEmpty<ImageData>("images"_L1, readImageData);
	tool.time = fields.integerOr("time"_L1, 0);
	tool.timeEnd = fields.optionalInteger("time_end"_L1);
	tool.runAt = fields.optionalInteger("run_at"_L1);
	tool.providerData = fields.value("provider_data"_L1);
	return tool;
}

} // namespace

ImageData readImageData(const Fields& fields)
{
	ImageData image{.mime = fields.string("mime"_L1), .data = fields.string("data"_L1), .id = fields.optionalString("id"_L1)};
	if(const std::optional<qint64> size = fields.optionalInteger("size"_L1))
	{
		image.size = static_cast<quint64>(*size);
	}
	return image;
}

ToolState readToolState(const QString& path, const QJsonValue& value)
{
	static constexpr std::array<std::pair<QLatin1StringView, ToolState>, 8> names{{
		{"streaming"_L1, ToolState::Streaming},
		{"awaiting_permission"_L1, ToolState::AwaitingPermission},
		{"delayed"_L1, ToolState::Delayed},
		{"running"_L1, ToolState::Running},
		{"done"_L1, ToolState::Done},
		{"error"_L1, ToolState::Error},
		{"denied"_L1, ToolState::Denied},
		{"cancelled"_L1, ToolState::Cancelled},
	}};
	if(!value.isString())
	{
		fail(path, u"expected a string"_s);
	}
	return readEnum(path, value.toString(), names);
}

ModelRef readModelRef(const Fields& fields)
{
	return {.provider = fields.string("provider"_L1), .model = fields.string("model"_L1), .variant = fields.optionalString("variant"_L1)};
}

SessionInfo readSessionInfo(const Fields& fields)
{
	SessionInfo info;
	info.id = fields.string("id"_L1);
	info.name = fields.string("name"_L1);
	info.title = fields.string("title"_L1);
	info.agent = fields.string("agent"_L1);
	info.model = readModelRef(Fields(fields.object("model"_L1), fields.childPath("model"_L1)));
	info.cwd = fields.string("cwd"_L1);
	info.createdAt = fields.integer("created_at"_L1);
	info.updatedAt = fields.integer("updated_at"_L1);
	info.status = readSessionStatus(Fields(fields.object("status"_L1), fields.childPath("status"_L1)));
	info.attached = static_cast<quint32>(fields.integer("attached"_L1));
	info.live = fields.booleanOr("live"_L1, false);
	info.parent = fields.optionalString("parent"_L1);
	info.mustReport = fields.booleanOr("must_report"_L1, false);
	if(const std::optional<QJsonObject> context = fields.optionalObject("context"_L1))
	{
		info.context = readContextUsage(Fields(*context, fields.childPath("context"_L1)));
	}
	info.todos = fields.listOrEmpty<Todo>("todos"_L1, [](const Fields& todo) {
		return Todo{.content = todo.string("content"_L1), .status = readTodoStatus(todo)};
	});
	info.busySince = fields.optionalInteger("busy_since"_L1);
	info.lastError = fields.optionalString("last_error"_L1);
	return info;
}

Part readPart(const Fields& fields)
{
	const QString type = fields.string("type"_L1);
	if(type == "text"_L1)
	{
		return TextPart{.text = fields.string("text"_L1), .time = fields.integerOr("time"_L1, 0)};
	}
	if(type == "reasoning"_L1)
	{
		return ReasoningPart{.text = fields.string("text"_L1),
		                     .time = fields.integerOr("time"_L1, 0),
		                     .timeEnd = fields.optionalInteger("time_end"_L1),
		                     .providerData = fields.value("provider_data"_L1)};
	}
	if(type == "image"_L1)
	{
		return ImagePart{.image = readImageData(fields), .name = fields.optionalString("name"_L1)};
	}
	if(type == "tool"_L1)
	{
		return readToolPart(fields);
	}
	return UnknownPart{.type = type};
}

Message readMessage(const Fields& fields)
{
	static constexpr std::array<std::pair<QLatin1StringView, Role>, 2> roles{{
		{"user"_L1, Role::User},
		{"assistant"_L1, Role::Assistant},
	}};
	Message message;
	message.id = fields.string("id"_L1);
	message.role = readEnum(fields.childPath("role"_L1), fields.string("role"_L1), roles);
	message.time = fields.integer("time"_L1);
	message.timeEnd = fields.optionalInteger("time_end"_L1);
	if(const std::optional<QJsonObject> source = fields.optionalObject("source"_L1))
	{
		message.source = readMessageSource(Fields(*source, fields.childPath("source"_L1)));
	}
	message.parts = fields.list<Part>("parts"_L1, readPart);
	if(const std::optional<QJsonObject> model = fields.optionalObject("model"_L1))
	{
		message.model = readModelRef(Fields(*model, fields.childPath("model"_L1)));
	}
	message.agent = fields.optionalString("agent"_L1);
	if(const std::optional<QJsonObject> usage = fields.optionalObject("usage"_L1))
	{
		message.usage = readUsage(Fields(*usage, fields.childPath("usage"_L1)));
	}
	message.error = fields.optionalString("error"_L1);
	return message;
}

QList<Message> readMessages(const Fields& fields, QLatin1StringView key)
{
	return fields.list<Message>(key, readMessage);
}

NamedImage readNamedImage(const Fields& fields)
{
	return {.mime = fields.string("mime"_L1), .data = fields.string("data"_L1), .name = fields.optionalString("name"_L1)};
}

PermissionRequest readPermissionRequest(const Fields& fields)
{
	return {.sessionId = fields.string("session_id"_L1),
	        .messageId = fields.string("message_id"_L1),
	        .callId = fields.string("call_id"_L1),
	        .tool = fields.string("tool"_L1),
	        .purpose = fields.string("purpose"_L1),
	        .input = fields.value("input"_L1),
	        .time = fields.integer("time"_L1)};
}

} // namespace json

Parsed<Part> parsePart(const QJsonObject& part)
{
	return json::guarded([&] { return json::readPart(json::Fields(part, u"part"_s)); });
}

Parsed<Message> parseMessage(const QJsonObject& message)
{
	return json::guarded([&] { return json::readMessage(json::Fields(message, u"message"_s)); });
}

Parsed<ToolState> parseToolState(const QJsonValue& state)
{
	return json::guarded([&] { return json::readToolState(u"tool state"_s, state); });
}

Parsed<SessionInfo> parseSessionInfo(const QJsonObject& info)
{
	return json::guarded([&] { return json::readSessionInfo(json::Fields(info, u"session"_s)); });
}

Parsed<InstanceInfo> parseInstanceInfo(const QJsonObject& instance)
{
	return json::guarded([&] {
		const json::Fields fields(instance, u"instance"_s);
		return InstanceInfo{.pid = static_cast<quint32>(fields.integer("pid"_L1)),
		                    .port = static_cast<quint16>(fields.integer("port"_L1)),
		                    .bind = fields.string("bind"_L1),
		                    .startedAt = fields.integer("started_at"_L1),
		                    .token = fields.string("token"_L1)};
	});
}

} // namespace refgui::proto
