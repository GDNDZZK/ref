// Adapted from an early version of qutie by LightBlueCube
#include "session/conversation_model.h"

#include <QDateTime>

#include <utility>

using namespace Qt::StringLiterals;

namespace refgui::session
{
namespace
{

// Source names of the user messages that start a new turn; the others are interjections.
bool opensTurn(const Block& block)
{
	const bool isTurnSource = block.source == u"human"_s || block.source == u"peer"_s || block.source == u"spawn"_s;
	return block.kind == Block::Kind::User && isTurnSource;
}

QList<int> changedRoles(const Block& before, const Block& after)
{
	using Role = ConversationModel::Role;
	const std::pair<Role, bool> differences[] = {
		{Role::KindRole, before.kind != after.kind},
		{Role::ContentRole, before.content != after.content},
		{Role::LanguageRole, before.language != after.language},
		{Role::StreamingRole, before.streaming != after.streaming},
		{Role::OutputRole, before.output != after.output},
		{Role::FailedRole, before.failed != after.failed},
		{Role::MessageIdRole, before.messageId != after.messageId},
		{Role::PartIndexRole, before.partIndex != after.partIndex},
		{Role::SourceRole, before.source != after.source},
		{Role::SenderRole, before.sender != after.sender},
		{Role::OpensTurnRole, before.kind != after.kind || before.source != after.source},
		{Role::TimeRole, before.time != after.time},
		{Role::ToolNameRole, before.toolName != after.toolName},
		{Role::PurposeRole, before.purpose != after.purpose},
		{Role::ToolStateRole, before.toolState != after.toolState},
		{Role::InputJsonRole, before.inputJson != after.inputJson},
		{Role::RawInputRole, before.rawInput != after.rawInput},
		{Role::CallIdRole, before.callId != after.callId},
	};
	QList<int> roles;
	for(const auto& [role, differs] : differences)
	{
		if(differs)
		{
			roles.append(role);
		}
	}
	return roles;
}

// What a delta for a part not seen yet creates, same as the terminal client does.
proto::Part newPartFor(proto::Delta::Kind kind, qsizetype index)
{
	const qint64 now = QDateTime::currentMSecsSinceEpoch();
	switch(kind)
	{
	case proto::Delta::Kind::Text:
		return proto::TextPart{.text = {}, .time = now};
	case proto::Delta::Kind::Reasoning:
		return proto::ReasoningPart{.text = {}, .time = now};
	case proto::Delta::Kind::ToolInput:
		return proto::ToolPart{.callId = u"pending-%1"_s.arg(index), .name = {}, .time = now};
	}
	Q_UNREACHABLE_RETURN(proto::TextPart{});
}

QList<proto::Part> partsOf(const auto& nodes)
{
	QList<proto::Part> parts;
	for(const auto& node : nodes)
	{
		parts.append(node.part);
	}
	return parts;
}

} // namespace

int ConversationModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_rows.size());
}

QVariant ConversationModel::data(const QModelIndex& index, int role) const
{
	Q_ASSERT(checkIndex(index, CheckIndexOption::IndexIsValid | CheckIndexOption::ParentIsInvalid));
	const Block& block = m_rows.at(index.row());

	switch(role)
	{
	case KindRole:
		return kindName(block.kind);
	case ContentRole:
		return block.content;
	case LanguageRole:
		return block.language;
	case StreamingRole:
		return block.streaming;
	case OutputRole:
		return block.output;
	case FailedRole:
		return block.failed;
	case MessageIdRole:
		return block.messageId;
	case PartIndexRole:
		return static_cast<int>(block.partIndex);
	case SourceRole:
		return block.source;
	case SenderRole:
		return block.sender;
	case OpensTurnRole:
		return index.row() > 0 && opensTurn(block);
	case TimeRole:
		return block.time;
	case ToolNameRole:
		return block.toolName;
	case PurposeRole:
		return block.purpose;
	case ToolStateRole:
		return block.toolState;
	case InputJsonRole:
		return block.inputJson;
	case RawInputRole:
		return block.rawInput;
	case CallIdRole:
		return block.callId;
	default:
		return {};
	}
}

QHash<int, QByteArray> ConversationModel::roleNames() const
{
	return {
		{KindRole, "kind"},
		{ContentRole, "content"},
		{LanguageRole, "language"},
		{StreamingRole, "streaming"},
		{OutputRole, "output"},
		{FailedRole, "failed"},
		{MessageIdRole, "messageId"},
		{PartIndexRole, "partIndex"},
		{SourceRole, "source"},
		{SenderRole, "sender"},
		{OpensTurnRole, "opensTurn"},
		{TimeRole, "time"},
		{ToolNameRole, "toolName"},
		{PurposeRole, "purpose"},
		{ToolStateRole, "toolState"},
		{InputJsonRole, "inputJson"},
		{RawInputRole, "rawInput"},
		{CallIdRole, "callId"},
	};
}

void ConversationModel::reset(const QList<proto::Message>& messages)
{
	QList<MessageNode> nodes;
	QList<Block> rows;
	for(const proto::Message& message : messages)
	{
		MessageNode node = makeNode(message);
		rows += buildRows(node);
		nodes.append(std::move(node));
	}

	beginResetModel();
	m_messages = std::move(nodes);
	m_rows = std::move(rows);
	endResetModel();
}

void ConversationModel::prependMessages(const QList<proto::Message>& older)
{
	QList<MessageNode> nodes;
	QList<Block> rows;
	for(const proto::Message& message : older)
	{
		MessageNode node = makeNode(message);
		rows += buildRows(node);
		nodes.append(std::move(node));
	}
	if(rows.isEmpty())
	{
		m_messages = nodes + m_messages;
		return;
	}

	const qsizetype added = rows.size();
	const bool hadRows = !m_rows.isEmpty();
	beginInsertRows({}, 0, static_cast<int>(added) - 1);
	m_messages = nodes + m_messages;
	m_rows = rows + m_rows;
	endInsertRows();

	if(hadRows)
	{
		notifyOpensTurn(added); // the former first row now has something above it
	}
}

void ConversationModel::setMessage(const proto::Message& message)
{
	const qsizetype existing = indexOf(message.id);
	if(existing >= 0)
	{
		replaceMessage(existing, message);
		return;
	}

	MessageNode node = makeNode(message);
	const QList<Block> rows = buildRows(node);
	const qsizetype first = m_rows.size();
	if(rows.isEmpty())
	{
		m_messages.append(std::move(node));
		return;
	}
	beginInsertRows({}, static_cast<int>(first), static_cast<int>(first + rows.size()) - 1);
	m_messages.append(std::move(node));
	m_rows += rows;
	endInsertRows();
}

bool ConversationModel::setPart(const QString& messageId, qsizetype index, const proto::Part& part)
{
	const qsizetype messageIndex = indexOf(messageId);
	if(messageIndex < 0)
	{
		return false;
	}

	// A gap in the indexes should not happen; appending keeps the order.
	QList<PartNode>& parts = m_messages[messageIndex].parts;
	const qsizetype partIndex = std::min(index, parts.size());
	if(partIndex < parts.size())
	{
		parts[partIndex].part = part;
	}
	else
	{
		parts.append({.part = part});
	}
	updateAfterPartChange(messageIndex, partIndex);
	return true;
}

bool ConversationModel::appendDelta(const QString& messageId, qsizetype index, const proto::Delta& delta)
{
	const qsizetype messageIndex = indexOf(messageId);
	if(messageIndex < 0)
	{
		return false;
	}

	QList<PartNode>& parts = m_messages[messageIndex].parts;
	while(parts.size() < index)
	{
		parts.append({.part = proto::TextPart{}}); // keeps the later indexes aligned
	}
	if(parts.size() == index)
	{
		parts.append({.part = newPartFor(delta.kind, index)});
	}

	proto::Part& part = parts[index].part;
	const bool isText = delta.kind != proto::Delta::Kind::ToolInput;
	if(auto* text = std::get_if<proto::TextPart>(&part); text && isText)
	{
		text->text += delta.text;
	}
	else if(auto* reasoning = std::get_if<proto::ReasoningPart>(&part); reasoning && isText)
	{
		reasoning->text += delta.text;
	}
	else if(auto* tool = std::get_if<proto::ToolPart>(&part); tool && !isText)
	{
		tool->rawInput += delta.text;
		if(tool->purpose.isEmpty())
		{
			tool->purpose = partialPurpose(tool->rawInput);
		}
	}
	else
	{
		return true; // a delta that does not fit its part has nothing to show
	}

	const MessageNode& node = m_messages.at(messageIndex);
	const bool keepsStreaming = parts[index].streaming && isPartStreaming(node.header, part, index == parts.size() - 1);
	if(node.header.role == proto::Role::Assistant && std::holds_alternative<proto::TextPart>(part) && keepsStreaming)
	{
		feedPart(messageIndex, index, delta.text);
		refresh(messageIndex);
		return true;
	}
	updateAfterPartChange(messageIndex, index);
	return true;
}

void ConversationModel::removeMessage(const QString& messageId)
{
	const qsizetype messageIndex = indexOf(messageId);
	if(messageIndex < 0)
	{
		return;
	}

	const qsizetype first = firstRowOf(messageIndex);
	const qsizetype count = m_messages.at(messageIndex).rows();
	if(count == 0)
	{
		m_messages.remove(messageIndex);
		return;
	}
	beginRemoveRows({}, static_cast<int>(first), static_cast<int>(first + count) - 1);
	m_messages.remove(messageIndex);
	m_rows.remove(first, count);
	endRemoveRows();

	if(first == 0 && !m_rows.isEmpty())
	{
		notifyOpensTurn(0);
	}
}

std::optional<QString> ConversationModel::partText(const QString& messageId, qsizetype index) const
{
	const qsizetype messageIndex = indexOf(messageId);
	if(messageIndex < 0 || index >= m_messages.at(messageIndex).parts.size())
	{
		return std::nullopt;
	}
	const proto::Part& part = m_messages.at(messageIndex).parts.at(index).part;
	if(const auto* text = std::get_if<proto::TextPart>(&part))
	{
		return text->text;
	}
	if(const auto* reasoning = std::get_if<proto::ReasoningPart>(&part))
	{
		return reasoning->text;
	}
	return std::nullopt;
}

qsizetype ConversationModel::partCount(const QString& messageId) const
{
	const qsizetype messageIndex = indexOf(messageId);
	return messageIndex < 0 ? -1 : m_messages.at(messageIndex).parts.size();
}

ConversationModel::MessageNode ConversationModel::makeNode(proto::Message message)
{
	MessageNode node;
	for(proto::Part& part : message.parts)
	{
		node.parts.append({.part = std::move(part)});
	}
	message.parts.clear();
	node.header = std::move(message);
	return node;
}

// Most updates are at the end of the conversation, so search from there.
qsizetype ConversationModel::indexOf(const QString& messageId) const
{
	for(qsizetype i = m_messages.size() - 1; i >= 0; --i)
	{
		if(m_messages.at(i).header.id == messageId)
		{
			return i;
		}
	}
	return -1;
}

qsizetype ConversationModel::firstRowOf(qsizetype messageIndex) const
{
	qsizetype first = m_rows.size();
	for(qsizetype i = m_messages.size() - 1; i >= messageIndex; --i)
	{
		first -= m_messages.at(i).rows();
	}
	return first;
}

qsizetype ConversationModel::firstRowOfPart(qsizetype messageIndex, qsizetype partIndex) const
{
	qsizetype first = firstRowOf(messageIndex);
	const QList<PartNode>& parts = m_messages.at(messageIndex).parts;
	for(qsizetype i = 0; i < partIndex; ++i)
	{
		first += parts.at(i).rows;
	}
	return first;
}

QList<Block> ConversationModel::buildRows(MessageNode& node)
{
	node.partRows = 0;
	if(node.header.role == proto::Role::User)
	{
		const QList<Block> rows = userBlocks(node.header, partsOf(node.parts));
		node.ownRows = rows.size();
		return rows;
	}

	QList<Block> rows;
	for(qsizetype i = 0; i < node.parts.size(); ++i)
	{
		PartNode& part = node.parts[i];
		part.streaming = isPartStreaming(node.header, part.part, i == node.parts.size() - 1);
		part.splitter = {};
		const QList<Block> blocks = assistantPartBlocks(node.header, i, part.part, part.streaming, part.splitter);
		part.rows = blocks.size();
		node.partRows += part.rows;
		rows += blocks;
	}
	const QList<Block> error = errorBlocks(node.header);
	node.ownRows = error.size();
	return rows + error;
}

void ConversationModel::rebuildPart(qsizetype messageIndex, qsizetype partIndex)
{
	MessageNode& node = m_messages[messageIndex];
	PartNode& part = node.parts[partIndex];
	const qsizetype first = firstRowOfPart(messageIndex, partIndex);

	part.streaming = isPartStreaming(node.header, part.part, partIndex == node.parts.size() - 1);
	part.splitter = {};
	const QList<Block> blocks = assistantPartBlocks(node.header, partIndex, part.part, part.streaming, part.splitter);
	replaceRows(first, part.rows, blocks);
	node.partRows += blocks.size() - part.rows;
	part.rows = blocks.size();
}

void ConversationModel::rebuildOwnRows(qsizetype messageIndex)
{
	MessageNode& node = m_messages[messageIndex];
	const qsizetype first = firstRowOf(messageIndex) + node.partRows;

	const bool isUser = node.header.role == proto::Role::User;
	const QList<Block> rows = isUser ? userBlocks(node.header, partsOf(node.parts)) : errorBlocks(node.header);
	replaceRows(first, node.ownRows, rows);
	node.ownRows = rows.size();
}

void ConversationModel::refresh(qsizetype messageIndex)
{
	const MessageNode& node = m_messages.at(messageIndex);
	for(qsizetype i = 0; i < node.parts.size(); ++i)
	{
		const bool streaming = isPartStreaming(node.header, node.parts.at(i).part, i == node.parts.size() - 1);
		if(streaming != node.parts.at(i).streaming)
		{
			rebuildPart(messageIndex, i);
		}
	}
}

// The new rows are diffed against the old ones as a whole: a message has few blocks, and
// a change in the middle is rare.
void ConversationModel::replaceMessage(qsizetype messageIndex, proto::Message message)
{
	const qsizetype first = firstRowOf(messageIndex);
	const qsizetype oldCount = m_messages.at(messageIndex).rows();

	MessageNode node = makeNode(std::move(message));
	const QList<Block> rows = buildRows(node);
	m_messages[messageIndex] = std::move(node);
	replaceRows(first, oldCount, rows);
}

void ConversationModel::feedPart(qsizetype messageIndex, qsizetype partIndex, const QString& text)
{
	MessageNode& node = m_messages[messageIndex];
	PartNode& part = node.parts[partIndex];
	const qsizetype first = firstRowOfPart(messageIndex, partIndex);

	QList<Block> blocks = m_rows.mid(first, part.rows);
	feedText(blocks, part.splitter, node.header, partIndex, text);
	replaceRows(first, part.rows, blocks);
	node.partRows += blocks.size() - part.rows;
	part.rows = blocks.size();
}

void ConversationModel::updateAfterPartChange(qsizetype messageIndex, qsizetype partIndex)
{
	if(m_messages.at(messageIndex).header.role == proto::Role::User)
	{
		rebuildOwnRows(messageIndex);
		return;
	}
	rebuildPart(messageIndex, partIndex);
	refresh(messageIndex); // a new part ends the streaming of the one before
}

void ConversationModel::replaceRows(qsizetype first, qsizetype oldCount, const QList<Block>& fresh)
{
	const qsizetype common = std::min(oldCount, fresh.size());
	for(qsizetype i = 0; i < common; ++i)
	{
		const QList<int> roles = changedRoles(m_rows.at(first + i), fresh.at(i));
		if(roles.isEmpty())
		{
			continue;
		}
		m_rows[first + i] = fresh.at(i);
		const QModelIndex changed = index(static_cast<int>(first + i));
		emit dataChanged(changed, changed, roles);
	}

	if(fresh.size() > oldCount)
	{
		beginInsertRows({}, static_cast<int>(first + oldCount), static_cast<int>(first + fresh.size()) - 1);
		for(qsizetype i = oldCount; i < fresh.size(); ++i)
		{
			m_rows.insert(first + i, fresh.at(i));
		}
		endInsertRows();
	}
	else if(fresh.size() < oldCount)
	{
		beginRemoveRows({}, static_cast<int>(first + fresh.size()), static_cast<int>(first + oldCount) - 1);
		m_rows.remove(first + fresh.size(), oldCount - fresh.size());
		endRemoveRows();
	}
}

void ConversationModel::notifyOpensTurn(qsizetype row)
{
	const QModelIndex changed = index(static_cast<int>(row));
	emit dataChanged(changed, changed, {OpensTurnRole});
}

} // namespace refgui::session
