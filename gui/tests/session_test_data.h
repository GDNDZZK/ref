#pragma once

#include "proto/types.h"
#include "session/conversation_model.h"

#include <QStringList>

// Builders for protocol messages, and a text rendering of the model, shared by the
// session layer tests.
namespace refgui::test
{

using namespace Qt::StringLiterals;

// When the built messages and parts start and end.
constexpr qint64 StartTime = 1;
constexpr qint64 EndTime = 2;

inline proto::Part text(const QString& text)
{
	return proto::TextPart{.text = text, .time = StartTime};
}

inline proto::Part reasoning(const QString& text, bool ended = true)
{
	return proto::ReasoningPart{.text = text, .time = StartTime, .timeEnd = ended ? std::optional<qint64>(EndTime) : std::nullopt};
}

inline proto::Part tool(const QString& name, const QString& purpose, proto::ToolState state, const QString& output = {})
{
	return proto::ToolPart{.callId = u"call-" + name,
	                       .name = name,
	                       .purpose = purpose,
	                       .input = QJsonObject{{u"command"_s, u"ls"_s}},
	                       .state = state,
	                       .output = output.isEmpty() ? std::nullopt : std::optional<QString>(output),
	                       .time = StartTime};
}

inline proto::Message user(const QString& id, const QString& text, proto::MessageSource source = {})
{
	return {.id = id, .role = proto::Role::User, .time = StartTime, .source = source, .parts = {proto::TextPart{.text = text, .time = StartTime}}};
}

// An assistant message; `ended` is false while the turn is still streaming.
inline proto::Message assistant(const QString& id, const QList<proto::Part>& parts, bool ended = true)
{
	return {.id = id, .role = proto::Role::Assistant, .time = StartTime, .timeEnd = ended ? std::optional<qint64>(EndTime) : std::nullopt, .parts = parts};
}

// Renders every block but the meta lines as "kind:content", plus "@language" for code blocks.
inline QStringList render(const session::ConversationModel& model)
{
	QStringList rendered;
	for(int row = 0; row < model.rowCount(); ++row)
	{
		const QModelIndex index = model.index(row);
		const QString kind = index.data(session::ConversationModel::KindRole).toString();
		if(kind == session::kindName(session::Block::Kind::Meta))
		{
			continue; // the line under a message has its own tests
		}
		QString block = kind + u':' + index.data(session::ConversationModel::ContentRole).toString();
		if(kind == session::kindName(session::Block::Kind::Code))
		{
			block += u'@' + index.data(session::ConversationModel::LanguageRole).toString();
		}
		rendered.append(block);
	}
	return rendered;
}

// A role of a row, counting the rows without the meta lines (like `render`).
inline QVariant roleOf(const session::ConversationModel& model, int row, int role)
{
	int contentRow = -1;
	for(int index = 0; index < model.rowCount(); ++index)
	{
		const bool isMeta = model.index(index).data(session::ConversationModel::KindRole).toString() == session::kindName(session::Block::Kind::Meta);
		if(!isMeta && ++contentRow == row)
		{
			return model.index(index).data(role);
		}
	}
	return {};
}

// The number of rows without the meta lines.
inline int contentRowCount(const session::ConversationModel& model)
{
	return static_cast<int>(render(model).size());
}

} // namespace refgui::test
