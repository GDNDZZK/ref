// Adapted from an early version of qutie by LightBlueCube
#include "session/conversation_model.h"

#include <utility>

using namespace Qt::StringLiterals;

namespace refgui::session
{

int ConversationModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_blocks.size());
}

QVariant ConversationModel::data(const QModelIndex& index, int role) const
{
	Q_ASSERT(checkIndex(index, CheckIndexOption::IndexIsValid | CheckIndexOption::ParentIsInvalid));
	const Block& block = m_blocks.at(index.row());

	switch(role)
	{
	case KindRole:
		return kindName(block.kind);
	case ContentRole:
		return block.content;
	case LanguageRole:
		return block.language;
	case StreamingRole:
		return m_streamingRow == index.row();
	case OutputRole:
		return block.output;
	case FailedRole:
		return block.failed;
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
	};
}

void ConversationModel::appendUserMessage(const QString& text)
{
	Q_ASSERT(!m_streamingRow);
	insertBlock({.kind = Kind::User, .content = text}, false);
}

void ConversationModel::appendReasoning(QStringView delta)
{
	if(!isStreaming(Kind::Reasoning))
	{
		endTextSegment();
		startStreamingBlock({.kind = Kind::Reasoning});
	}
	appendToStreamingBlock(delta);
}

void ConversationModel::appendAssistantText(QStringView delta)
{
	for(const FenceSplitter::Event& event : m_splitter.feed(delta))
	{
		applySplitEvent(event);
	}
}

void ConversationModel::beginToolCall(const QString& title)
{
	endTextSegment();
	startStreamingBlock({.kind = Kind::Tool, .content = title});
}

void ConversationModel::endToolCall(bool succeeded, const QString& output)
{
	Q_ASSERT(isStreaming(Kind::Tool));
	Block& block = m_blocks[*m_streamingRow];
	block.output = output;
	block.failed = !succeeded;
	notifyChanged(*m_streamingRow, {OutputRole, FailedRole});
	finishStreamingBlock();
}

void ConversationModel::endAssistantTurn()
{
	endTextSegment();
	finishStreamingBlock();
}

void ConversationModel::appendError(const QString& message)
{
	Q_ASSERT(!m_streamingRow);
	insertBlock({.kind = Kind::Error, .content = message}, false);
}

QString ConversationModel::kindName(Kind kind)
{
	switch(kind)
	{
	case Kind::User:
		return u"user"_s;
	case Kind::Reasoning:
		return u"reasoning"_s;
	case Kind::Markdown:
		return u"markdown"_s;
	case Kind::Code:
		return u"code"_s;
	case Kind::Tool:
		return u"tool"_s;
	case Kind::Error:
		return u"error"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

void ConversationModel::applySplitEvent(const FenceSplitter::Event& event)
{
	if(const auto* opened = std::get_if<FenceSplitter::CodeOpened>(&event))
	{
		startStreamingBlock({.kind = Kind::Code, .language = opened->language});
		return;
	}
	if(std::holds_alternative<FenceSplitter::CodeClosed>(event))
	{
		finishStreamingBlock();
		return;
	}

	// Prose after a closed fence, reasoning or a tool call starts a new Markdown block.
	const bool continuesTextBlock = isStreaming(Kind::Markdown) || isStreaming(Kind::Code);
	if(!continuesTextBlock)
	{
		startStreamingBlock({.kind = Kind::Markdown});
	}
	appendToStreamingBlock(std::get<FenceSplitter::TextAppended>(event).text);
}

// Releases the splitter's held-back line and starts the next segment outside any fence.
void ConversationModel::endTextSegment()
{
	for(const FenceSplitter::Event& event : m_splitter.finish())
	{
		applySplitEvent(event);
	}
	m_splitter = {};
}

void ConversationModel::insertBlock(Block block, bool streaming)
{
	const qsizetype row = m_blocks.size();
	beginInsertRows({}, static_cast<int>(row), static_cast<int>(row));
	m_blocks.append(std::move(block));
	if(streaming)
	{
		m_streamingRow = row;
	}
	endInsertRows();
}

void ConversationModel::startStreamingBlock(Block block)
{
	finishStreamingBlock();
	insertBlock(std::move(block), true);
}

void ConversationModel::appendToStreamingBlock(QStringView text)
{
	Q_ASSERT(m_streamingRow);
	m_blocks[*m_streamingRow].content.append(text);
	notifyChanged(*m_streamingRow, {ContentRole});
}

void ConversationModel::finishStreamingBlock()
{
	const std::optional<qsizetype> row = std::exchange(m_streamingRow, std::nullopt);
	if(row)
	{
		notifyChanged(*row, {StreamingRole});
	}
}

bool ConversationModel::isStreaming(Kind kind) const
{
	return m_streamingRow && m_blocks.at(*m_streamingRow).kind == kind;
}

void ConversationModel::notifyChanged(qsizetype row, const QList<int>& roles)
{
	const QModelIndex changed = index(static_cast<int>(row));
	emit dataChanged(changed, changed, roles);
}

} // namespace refgui::session
