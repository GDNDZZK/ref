// Adapted from an early version of LightBlueCube/qutie
#include "session/conversation_model.h"

#include "session/markdown_text.h"
#include "session/layout_estimator.h"

#include <QDateTime>

#include <algorithm>

#include <utility>

using namespace Qt::StringLiterals;

namespace refgui::session
{
namespace
{

using SourceKind = proto::MessageSource::Kind;

// Whether the views draw the content as Markdown: they then must not get it raw.
bool rendersMarkdown(const Block& block)
{
	const bool markdownSource = block.kind == Block::Kind::User && (block.sourceKind == SourceKind::Shell || block.sourceKind == SourceKind::Compaction);
	return block.kind == Block::Kind::Markdown || block.kind == Block::Kind::Reasoning || markdownSource;
}

bool opensTurn(const Block& block)
{
	// User messages from these start a new turn; the others are interjections.
	const bool isTurnSource = block.sourceKind == SourceKind::Human || block.sourceKind == SourceKind::Peer || block.sourceKind == SourceKind::Spawn;
	return block.kind == Block::Kind::User && isTurnSource;
}

QList<int> changedRoles(const Block& before, const Block& after)
{
	using Role = ConversationModel::Role;
	const std::pair<Role, bool> differences[] = {
		{Role::KindRole, before.kind != after.kind},
		{Role::ContentRole, before.content != after.content},
		{Role::LanguageRole, before.language != after.language},
		{Role::TableHeadersRole, before.tableHeaders != after.tableHeaders},
		{Role::TableAlignmentsRole, before.tableAlignments != after.tableAlignments},
		{Role::TableRowsRole, before.tableRows != after.tableRows},
		{Role::StreamingRole, before.streaming != after.streaming},
		{Role::OutputRole, before.output != after.output},
		{Role::FailedRole, before.failed != after.failed},
		{Role::MessageIdRole, before.messageId != after.messageId},
		{Role::PartIndexRole, before.partIndex != after.partIndex},
		{Role::SourceRole, before.sourceKind != after.sourceKind},
		{Role::SenderRole, before.sender != after.sender},
		{Role::OpensTurnRole, before.kind != after.kind || before.sourceKind != after.sourceKind},
		{Role::TimeRole, before.time != after.time},
		{Role::ToolNameRole, before.toolName != after.toolName},
		{Role::PurposeRole, before.purpose != after.purpose},
		{Role::ToolStateRole, before.toolStateKind != after.toolStateKind},
		{Role::InputJsonRole, before.inputJson != after.inputJson},
		{Role::RawInputRole, before.rawInput != after.rawInput},
		{Role::CallIdRole, before.callId != after.callId},
		{Role::TimeEndRole, before.timeEnd != after.timeEnd},
		{Role::RunAtRole, before.runAt != after.runAt},
		{Role::MetaRole, before.meta != after.meta},
		{Role::LiveOutputRole, before.liveOutput != after.liveOutput},
		{Role::WarningsRole, before.warnings != after.warnings},
		{Role::ImagesRole, before.images != after.images},
		{Role::AgentRole, before.agent != after.agent},
		{Role::ModelIdRole, before.modelId != after.modelId},
		{Role::VariantRole, before.variant != after.variant},
		{Role::TokensInRole, before.tokensIn != after.tokensIn},
		{Role::TokensOutRole, before.tokensOut != after.tokensOut},
		{Role::InterruptedRole, before.interrupted != after.interrupted},
		{Role::ErroredRole, before.errored != after.errored},
		{Role::HumanRole, before.human != after.human},
		{Role::ExpandedRole, before.kind != after.kind || before.sourceKind != after.sourceKind
			|| (before.kind == Block::Kind::Reasoning && before.streaming != after.streaming)},
		{Role::SeqRole, before.seq != after.seq},
		{Role::EstimatedHeightRole, before.kind == Block::Kind::Skeleton || after.kind == Block::Kind::Skeleton},
		{Role::SkeletonPartsRole, before.skeletonMeta != after.skeletonMeta},
		{Role::MessageRoleRole, before.skeletonMeta != after.skeletonMeta},
		{Role::HasErrorRole, before.skeletonMeta != after.skeletonMeta},
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
QVariantMap previewToVariant(const proto::EditPreview& preview)
{
	return {{u"diff"_s, preview.diff}, {u"added"_s, preview.added}, {u"removed"_s, preview.removed}, {u"truncated"_s, preview.truncated}};
}


QVariantList imagesToVariant(const QList<Block::ImageRef>& images)
{
	QVariantList list;
	for(const Block::ImageRef& image : images)
	{
		list.append(QVariantMap{{u"id"_s, image.id}, {u"mime"_s, image.mime}, {u"size"_s, image.size}, {u"name"_s, image.name}});
	}
	return list;
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

void ensureTableCache(const Block& block)
{
	const bool valid = block.tableCache.sourceHeaders == block.tableHeaders && block.tableCache.sourceRows == block.tableRows
	                   && block.tableCache.renderedHeaders.size() == block.tableHeaders.size();
	if(valid)
	{
		return;
	}
	block.tableCache.sourceHeaders = block.tableHeaders;
	block.tableCache.sourceRows = block.tableRows;
	block.tableCache.renderedHeaders.clear();
	block.tableCache.renderedHeaders.reserve(block.tableHeaders.size());
	for(const QString& cell : block.tableHeaders)
	{
		block.tableCache.renderedHeaders.append(markdownForDisplay(cell));
	}
	block.tableCache.renderedRows.clear();
	block.tableCache.renderedRows.reserve(block.tableRows.size());
	for(const QStringList& row : block.tableRows)
	{
		QStringList renderedRow;
		renderedRow.reserve(row.size());
		for(const QString& cell : row)
		{
			renderedRow.append(markdownForDisplay(cell));
		}
		block.tableCache.renderedRows.append(renderedRow);
	}
}

} // namespace

int ConversationModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_rows.size()) + (m_tailShown ? 1 : 0);
}

// The tail error is the row after the last of `m_rows`.
const Block& ConversationModel::blockAt(qsizetype row) const
{
	return row < m_rows.size() ? m_rows.at(row) : m_tailBlock;
}

bool ConversationModel::isExpandedByDefault(const Block& block) const
{
	switch(block.kind)
	{
	case Block::Kind::Reasoning:
		return block.streaming || m_reasoningExpanded;
	case Block::Kind::Tool:
		return m_toolsExpanded;
	case Block::Kind::User:
	case Block::Kind::Markdown:
	case Block::Kind::Table:
	case Block::Kind::Code:
	case Block::Kind::Error:
	case Block::Kind::Image:
	case Block::Kind::Meta:
	case Block::Kind::Skeleton:
		return false;
	}
	Q_UNREACHABLE_RETURN(false);
}

bool ConversationModel::isExpanded(const Block& block) const
{
	const bool isCompaction = block.kind == Block::Kind::User && block.sourceKind == SourceKind::Compaction;
	const bool foldable = isCompaction || block.kind == Block::Kind::Reasoning || block.kind == Block::Kind::Tool;
	if(!foldable)
	{
		return false;
	}
	const PartKey key{block.messageId, block.partIndex};
	if(const auto it = m_expandedOverrides.constFind(key); it != m_expandedOverrides.constEnd())
	{
		return it.value();
	}
	return isExpandedByDefault(block);
}

QVariant ConversationModel::data(const QModelIndex& index, int role) const
{
	Q_ASSERT(checkIndex(index, CheckIndexOption::IndexIsValid | CheckIndexOption::ParentIsInvalid));
	const Block& block = blockAt(index.row());

	switch(role)
	{
	case KindRole:
		return kindName(block.kind);
	case ContentRole:
		if(!rendersMarkdown(block))
		{
			return block.content;
		}
		if(block.markdownCache.rendered.isNull() || block.markdownCache.source != block.content)
		{
			block.markdownCache.source = block.content;
			block.markdownCache.rendered = markdownForDisplay(block.content);
		}
		return block.markdownCache.rendered;
	case LanguageRole:
		return block.language;
	case TableHeadersRole:
		ensureTableCache(block);
		return block.tableCache.renderedHeaders;
	case TableAlignmentsRole:
		return block.tableAlignments;
	case TableRowsRole:
		ensureTableCache(block);
		return block.tableCache.renderedRows;
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
		return sourceName(block.sourceKind);
	case SenderRole:
		return block.sender;
	case OpensTurnRole:
		if(block.kind == Block::Kind::Skeleton && block.skeletonMeta)
		{
			const QString& sk = block.skeletonMeta->sourceKind;
			const bool isTurnSource = sk.isEmpty() || sk == "human"_L1 || sk == "peer"_L1 || sk == "spawn"_L1;
			return index.row() > 0 && block.skeletonMeta->role == proto::Role::User && isTurnSource;
		}
		return index.row() > 0 && opensTurn(block);
	case ToolNameRole:
		return block.toolName;
	case PurposeRole:
		return block.purpose;
	case ToolStateRole:
		return toolStateName(block.toolStateKind);
	case InputJsonRole:
		return block.inputJson;
	case RawInputRole:
		return block.rawInput;
	case CallIdRole:
		return block.callId;
	case TimeEndRole:
		return block.timeEnd;
	case RunAtRole:
		return block.runAt;
	case MetaRole:
		return block.meta.toVariantMap();
	case LiveOutputRole:
		return block.liveOutput;
	case WarningsRole:
		return block.warnings;
	case PermissionPreviewRole:
		if(block.kind == Block::Kind::Tool && block.toolStateKind == proto::ToolState::AwaitingPermission && m_permissionPreviews.contains(block.callId))
		{
			return previewToVariant(m_permissionPreviews.value(block.callId));
		}
		return QVariantMap();
	case ImagesRole:
		return imagesToVariant(block.images);
	case AgentRole:
		return block.agent;
	case ModelIdRole:
		return block.modelId;
	case VariantRole:
		return block.variant;
	case TokensInRole:
		return block.tokensIn;
	case TokensOutRole:
		return block.tokensOut;
	case InterruptedRole:
		return block.interrupted;
	case ErroredRole:
		return block.errored;
	case HumanRole:
		return block.human;
	case StaleRole:
		return block.stale;
	case ContextStartRole:
		return block.contextStart;
	case ExpandedRole:
		return isExpanded(block);
	case ShowAllRole:
		return m_shownAll.contains({block.messageId, block.partIndex});
	case SeqRole:
		return static_cast<qulonglong>(block.seq);
	case EstimatedHeightRole:
		return estimatedHeightFor(block);
	case SkeletonPartsRole:
		return block.skeletonMeta ? LayoutEstimator::skeletonParts(*block.skeletonMeta) : QVariantList{};
	case MessageRoleRole:
		if(block.skeletonMeta)
		{
			return block.skeletonMeta->role == proto::Role::User ? u"user"_s : u"assistant"_s;
		}
		return QString();
	case HasErrorRole:
		return block.skeletonMeta ? block.skeletonMeta->hasError : false;
	default:
		return {};
	}
}

ConversationModel::ConversationModel(QObject* parent)
	: QAbstractListModel(parent)
{
	// Tool states change through row updates of any kind; the summary follows all of them.
	connect(this, &QAbstractItemModel::dataChanged, this, [this](const QModelIndex&, const QModelIndex&, const QList<int>& roles) {
		if(roles.isEmpty() || roles.contains(ToolStateRole))
		{
			refreshToolSummary();
		}
	});
	connect(this, &QAbstractItemModel::rowsInserted, this, &ConversationModel::refreshToolSummary);
	connect(this, &QAbstractItemModel::rowsRemoved, this, &ConversationModel::refreshToolSummary);
	connect(this, &QAbstractItemModel::modelReset, this, &ConversationModel::refreshToolSummary);
}

void ConversationModel::refreshToolSummary()
{
	ToolSummary summary;
	bool lastToolSeen = false;
	for(qsizetype i = m_rows.size() - 1; i >= 0; --i)
	{
		const Block& block = m_rows.at(i);
		if(block.kind != Block::Kind::Tool)
		{
			continue;
		}
		if(!lastToolSeen)
		{
			summary.lastToolDelayed = block.toolStateKind == proto::ToolState::Delayed;
			lastToolSeen = true;
		}
		if(block.toolStateKind == proto::ToolState::AwaitingPermission)
		{
			++summary.awaitingPermission;
			summary.oldestAwaitingCallId = block.callId; // the loop runs backwards: the last one set is the oldest
		}
	}
	if(summary == m_toolSummary)
	{
		return;
	}
	m_toolSummary = summary;
	emit toolSummaryChanged();
}

QHash<int, QByteArray> ConversationModel::roleNames() const
{
	return {
		{KindRole, "kind"},
		{ContentRole, "content"},
		{LanguageRole, "language"},
		{TableHeadersRole, "tableHeaders"},
		{TableAlignmentsRole, "tableAlignments"},
		{TableRowsRole, "tableRows"},
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
		{TimeEndRole, "timeEnd"},
		{RunAtRole, "runAt"},
		{MetaRole, "meta"},
		{LiveOutputRole, "liveOutput"},
		{WarningsRole, "warnings"},
		{PermissionPreviewRole, "permissionPreview"},
		{ImagesRole, "images"},
		{AgentRole, "agent"},
		{ModelIdRole, "modelId"},
		{VariantRole, "variant"},
		{TokensInRole, "tokensIn"},
		{TokensOutRole, "tokensOut"},
		{InterruptedRole, "interrupted"},
		{ErroredRole, "errored"},
		{HumanRole, "human"},
		{StaleRole, "stale"},
		{ContextStartRole, "contextStart"},
		{ExpandedRole, "expanded"},
		{ShowAllRole, "showAll"},
		{SeqRole, "seq"},
		{EstimatedHeightRole, "estimatedHeight"},
		{SkeletonPartsRole, "skeletonParts"},
		{MessageRoleRole, "messageRole"},
		{HasErrorRole, "hasError"},
	};
}
ConversationModel::Built ConversationModel::buildNodes(const QList<proto::Message>& messages,
                                                             quint64 totalMessages,
                                                             quint64 startSeq,
                                                             const QList<proto::MessageLayoutMeta>& layoutMetas)
{
	Built built;
	if(totalMessages == 0 && layoutMetas.isEmpty())
	{
		for(qsizetype i = 0; i < messages.size(); ++i)
		{
			MessageNode node = makeNode(messages.at(i));
			QList<Block> rows = buildRows(node);
			for(Block& b : rows)
			{
				b.seq = static_cast<quint64>(i);
			}
			built.rows += rows;
			built.nodes.append(std::move(node));
		}
		return built;
	}

	const quint64 total = std::max<quint64>(totalMessages, startSeq + static_cast<quint64>(messages.size()));
	QHash<quint64, proto::MessageLayoutMeta> metaBySeq;
	metaBySeq.reserve(layoutMetas.size());
	for(const proto::MessageLayoutMeta& meta : layoutMetas)
	{
		metaBySeq.insert(meta.seq, meta);
	}

	built.nodes.reserve(static_cast<qsizetype>(total));
	for(quint64 seq = 0; seq < total; ++seq)
	{
		if(seq >= startSeq && seq < startSeq + static_cast<quint64>(messages.size()))
		{
			const proto::Message& msg = messages.at(static_cast<qsizetype>(seq - startSeq));
			MessageNode node = makeNode(msg);
			if(auto it = metaBySeq.constFind(seq); it != metaBySeq.constEnd())
			{
				node.layoutMeta = *it;
			}
			else
			{
				node.layoutMeta = LayoutEstimator::computeMeta(seq, msg);
			}
			QList<Block> rows = buildRows(node);
			for(Block& b : rows)
			{
				b.seq = seq;
			}
			built.rows += rows;
			built.nodes.append(std::move(node));
		}
		else
		{
			proto::MessageLayoutMeta meta = metaBySeq.value(seq, proto::MessageLayoutMeta{
				.seq = seq,
				.id = u"placeholder-%1"_s.arg(seq),
				.role = proto::Role::Assistant,
				.sourceKind = {},
				.hasError = false,
				.parts = {},
			});
			MessageNode node = makePlaceholderNode(std::move(meta));
			Block skel;
			skel.kind = Block::Kind::Skeleton;
			skel.messageId = node.header.id;
			skel.partIndex = -1;
			skel.seq = seq;
			skel.sourceKind = node.layoutMeta ? sourceKindFromName(node.layoutMeta->sourceKind) : std::nullopt;
			skel.skeletonMeta = node.layoutMeta;
			node.ownRows = 1;
			built.rows.append(std::move(skel));
			built.nodes.append(std::move(node));
		}
	}
	return built;
}

void ConversationModel::reset(const QList<proto::Message>& messages,
                              quint64 totalMessages,
                              quint64 startSeq,
                              const QList<proto::MessageLayoutMeta>& layoutMetas)
{
	auto [nodes, rows] = buildNodes(messages, totalMessages, startSeq, layoutMetas);

	beginResetModel();
	m_messages = std::move(nodes);
	m_rows = std::move(rows);
	m_permissionPreviews.clear();
	endResetModel();
	refreshDerivedState();
}

void ConversationModel::replaceHistory(const QList<proto::Message>& messages,
                                       quint64 totalMessages,
                                       quint64 startSeq,
                                       const QList<proto::MessageLayoutMeta>& layoutMetas)
{
	auto [nodes, rows] = buildNodes(messages, totalMessages, startSeq, layoutMetas);

	const qsizetype comparable = std::min(m_rows.size(), rows.size());
	qsizetype kept = 0;
	while(kept < comparable && changedRoles(m_rows.at(kept), rows.at(kept)).isEmpty())
	{
		++kept;
	}

	if(kept < m_rows.size())
	{
		beginRemoveRows({}, static_cast<int>(kept), static_cast<int>(m_rows.size()) - 1);
		m_messages = nodes;
		m_rows.remove(kept, m_rows.size() - kept);
		endRemoveRows();
	}
	else
	{
		m_messages = nodes;
	}

	if(kept < rows.size())
	{
		beginInsertRows({}, static_cast<int>(kept), static_cast<int>(rows.size()) - 1);
		m_rows += rows.mid(kept);
		endInsertRows();
	}
	refreshDerivedState();
}
void ConversationModel::prependMessages(const QList<proto::Message>& older)
{
	auto [nodes, rows] = buildNodes(older);
	if(rows.isEmpty())
	{
		m_messages = nodes + m_messages;
		refreshDerivedState();
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
	refreshDerivedState();
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
	const quint64 seq = static_cast<quint64>(m_messages.size());
	m_messages.append(std::move(node));
	QList<Block> tagged = rows;
	for(Block& b : tagged)
	{
		b.seq = seq;
	}
	m_rows += tagged;
	endInsertRows();
	refreshDerivedState();
}
bool ConversationModel::setPart(const QString& messageId, qsizetype index, const proto::Part& part)
{
	const qsizetype messageIndex = indexOf(messageId);
	if(messageIndex < 0 || !m_messages.at(messageIndex).materialized)
	{
		return false;
	}
	m_messages[messageIndex].layoutMeta.reset();
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
	if(messageIndex < 0 || !m_messages.at(messageIndex).materialized)
	{
		return false;
	}
	m_messages[messageIndex].layoutMeta.reset();
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
	refreshDerivedState();
}
void ConversationModel::setPendingPermissions(const QList<proto::PermissionRequest>& requests)
{
	m_permissionPreviews.clear();
	for(const proto::PermissionRequest& request : requests)
	{
		if(request.preview)
		{
			m_permissionPreviews.insert(request.callId, *request.preview);
		}
	}
	for(qsizetype i = 0; i < m_rows.size(); ++i)
	{
		if(m_rows.at(i).kind == Block::Kind::Tool && m_rows.at(i).toolStateKind == proto::ToolState::AwaitingPermission)
		{
			emitRoleChanged(i, PermissionPreviewRole);
		}
	}
}

void ConversationModel::setPermission(const proto::PermissionRequest& request)
{
	if(request.preview)
	{
		m_permissionPreviews.insert(request.callId, *request.preview);
	}
	else
	{
		m_permissionPreviews.remove(request.callId);
	}
	for(qsizetype i = 0; i < m_rows.size(); ++i)
	{
		if(m_rows.at(i).kind == Block::Kind::Tool && m_rows.at(i).callId == request.callId)
		{
			emitRoleChanged(i, PermissionPreviewRole);
		}
	}
}

void ConversationModel::resolvePermission(const QString& callId)
{
	if(!m_permissionPreviews.remove(callId))
	{
		return;
	}
	for(qsizetype i = 0; i < m_rows.size(); ++i)
	{
		if(m_rows.at(i).kind == Block::Kind::Tool && m_rows.at(i).callId == callId)
		{
			emitRoleChanged(i, PermissionPreviewRole);
		}
	}
}


std::optional<QString> ConversationModel::partText(const QString& messageId, qsizetype index) const
{
	const qsizetype messageIndex = indexOf(messageId);
	if(messageIndex < 0 || !m_messages.at(messageIndex).materialized || index >= m_messages.at(messageIndex).parts.size())
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
	if(messageIndex < 0 || !m_messages.at(messageIndex).materialized)
	{
		return -1;
	}
	return m_messages.at(messageIndex).parts.size();
}
std::optional<QString> ConversationModel::messageText(const QString& messageId) const
{
	const qsizetype messageIndex = indexOf(messageId);
	if(messageIndex < 0)
	{
		return std::nullopt;
	}
	QStringList texts;
	for(const PartNode& node : m_messages.at(messageIndex).parts)
	{
		if(const auto* text = std::get_if<proto::TextPart>(&node.part))
		{
			texts.append(text->text);
		}
	}
	return texts.join(u"\n\n"_s);
}

QString ConversationModel::lastReplyId() const
{
	for(qsizetype i = m_messages.size() - 1; i >= 0; --i)
	{
		const MessageNode& node = m_messages.at(i);
		const auto isText = [](const PartNode& part) { return std::holds_alternative<proto::TextPart>(part.part); };
		if(node.header.role == proto::Role::Assistant && std::ranges::any_of(node.parts, isText))
		{
			return node.header.id;
		}
	}
	return {};
}

QString ConversationModel::firstMessageId() const
{
	return m_messages.isEmpty() ? QString() : m_messages.first().header.id;
}

void ConversationModel::setTailError(const QString& text)
{
	m_tailError = text;
	refreshTailError();
}

void ConversationModel::refreshTailError()
{
	const bool lastMessageShowsIt = !m_messages.isEmpty() && m_messages.last().header.error == m_tailError;
	const bool shown = !m_tailError.isEmpty() && !lastMessageShowsIt;
	const int row = static_cast<int>(m_rows.size());
	if(shown && !m_tailShown)
	{
		m_tailBlock = {.kind = Block::Kind::Error, .content = m_tailError, .partIndex = -1};
		beginInsertRows({}, row, row);
		m_tailShown = true;
		endInsertRows();
	}
	else if(!shown && m_tailShown)
	{
		beginRemoveRows({}, row, row);
		m_tailShown = false;
		endRemoveRows();
	}
	else if(shown && m_tailBlock.content != m_tailError)
	{
		m_tailBlock.content = m_tailError;
		emit dataChanged(index(row), index(row), {ContentRole});
	}
}

void ConversationModel::refreshDerivedState()
{
	refreshContext();
	refreshTailError();
}

void ConversationModel::setDefaultExpansion(bool reasoning, bool tools)
{
	// "Show all" and "hide all" overrule what the user flipped one by one.
	m_expandedOverrides.clear();
	m_reasoningExpanded = reasoning;
	m_toolsExpanded = tools;
	for(qsizetype row = 0; row < m_rows.size(); ++row)
	{
		const Block& block = m_rows.at(row);
		const bool isCompaction = block.kind == Block::Kind::User && block.sourceKind == SourceKind::Compaction;
		if(isCompaction || block.kind == Block::Kind::Reasoning || block.kind == Block::Kind::Tool)
		{
			emitRoleChanged(row, ExpandedRole);
		}
		else if(block.kind == Block::Kind::Skeleton)
		{
			emitRoleChanged(row, EstimatedHeightRole);
		}
	}
}
void ConversationModel::toggleExpanded(int row)
{
	if(row < 0 || row >= m_rows.size())
	{
		return;
	}
	const Block& block = m_rows.at(row);
	const PartKey key{block.messageId, block.partIndex};
	m_expandedOverrides.insert(key, !isExpanded(block));
	emitRoleChanged(row, ExpandedRole);
}

void ConversationModel::toggleShowAll(int row)
{
	if(row < 0 || row >= m_rows.size())
	{
		return;
	}
	const Block& block = m_rows.at(row);
	const PartKey key{block.messageId, block.partIndex};
	if(!m_shownAll.remove(key))
	{
		m_shownAll.insert(key);
	}
	emitRoleChanged(row, ShowAllRole);
}

void ConversationModel::emitRoleChanged(qsizetype row, int role)
{
	const QModelIndex changed = index(static_cast<int>(row));
	emit dataChanged(changed, changed, {role});
}

void ConversationModel::refreshContext()
{
	qsizetype lastCompaction = -1;
	for(qsizetype i = 0; i < m_messages.size(); ++i)
	{
		const MessageNode& node = m_messages.at(i);
		const bool isCompaction = node.materialized
			? (node.header.role == proto::Role::User && node.header.source.kind == proto::MessageSource::Kind::Compaction)
			: (node.layoutMeta && node.layoutMeta->role == proto::Role::User && node.layoutMeta->sourceKind == "compaction"_L1);
		if(isCompaction)
		{
			lastCompaction = i;
		}
	}
	qsizetype row = 0;
	for(qsizetype i = 0; i < m_messages.size(); ++i)
	{
		const bool stale = i < lastCompaction;
		const bool contextStart = i == lastCompaction && i > 0;
		for(qsizetype n = m_messages.at(i).rows(); n > 0; --n, ++row)
		{
			Block& block = m_rows[row];
			QList<int> roles;
			if(block.stale != stale)
			{
				roles.append(StaleRole);
			}
			if(block.contextStart != contextStart)
			{
				roles.append(ContextStartRole);
			}
			if(roles.isEmpty())
			{
				continue;
			}
			block.stale = stale;
			block.contextStart = contextStart;
			const QModelIndex changed = index(static_cast<int>(row));
			emit dataChanged(changed, changed, roles);
		}
	}
}

ConversationModel::MessageNode ConversationModel::makeNode(proto::Message message)
{
	MessageNode node;
	node.materialized = true;
	for(proto::Part& part : message.parts)
	{
		node.parts.append({.part = std::move(part)});
	}
	message.parts.clear();
	node.header = std::move(message);
	return node;
}

ConversationModel::MessageNode ConversationModel::makePlaceholderNode(proto::MessageLayoutMeta meta)
{
	MessageNode node;
	node.materialized = false;
	node.header.id = meta.id;
	node.header.role = meta.role;
	if(const auto sk = sourceKindFromName(meta.sourceKind))
	{
		node.header.source.kind = *sk;
	}
	node.header.timeEnd = 0;
	node.layoutMeta = std::move(meta);
	node.ownRows = 1;
	return node;
}

Block ConversationModel::makeSkeletonBlock(const proto::MessageLayoutMeta& meta) const
{
	Block block;
	block.kind = Block::Kind::Skeleton;
	block.messageId = meta.id;
	block.partIndex = -1;
	block.seq = meta.seq;
	block.sourceKind = sourceKindFromName(meta.sourceKind);
	block.skeletonMeta = meta;
	return block;
}

proto::Message ConversationModel::materializeHeaderAndParts(const MessageNode& node)
{
	proto::Message msg = node.header;
	msg.parts.reserve(node.parts.size());
	for(const PartNode& p : node.parts)
	{
		msg.parts.append(p.part);
	}
	return msg;
}

double ConversationModel::estimatedHeightFor(const Block& block) const
{
	if(block.kind != Block::Kind::Skeleton || !block.skeletonMeta)
	{
		return 0.0;
	}
	return LayoutEstimator::estimateMessageHeight(*block.skeletonMeta, m_columnWidth, m_reasoningExpanded, m_toolsExpanded);
}

void ConversationModel::setColumnWidth(double width)
{
	if(width <= 0.0 || qAbs(width - m_columnWidth) < 0.5)
	{
		return;
	}
	m_columnWidth = width;
	emitSkeletonHeightsChanged();
}

void ConversationModel::emitSkeletonHeightsChanged()
{
	for(qsizetype row = 0; row < m_rows.size(); ++row)
	{
		if(m_rows.at(row).kind == Block::Kind::Skeleton)
		{
			emitRoleChanged(row, EstimatedHeightRole);
		}
	}
}

bool ConversationModel::hasPlaceholders() const
{
	return std::ranges::any_of(m_messages, [](const MessageNode& n) { return !n.materialized; });
}

qsizetype ConversationModel::residentCount() const
{
	return std::ranges::count_if(m_messages, [](const MessageNode& n) { return n.materialized; });
}

int ConversationModel::firstRowForSeq(quint64 seq) const
{
	if(seq >= static_cast<quint64>(m_messages.size()))
	{
		return -1;
	}
	return static_cast<int>(firstRowOf(static_cast<qsizetype>(seq)));
}

int ConversationModel::seqForRow(int row) const
{
	if(row < 0 || row >= m_rows.size())
	{
		return -1;
	}
	qsizetype cursor = 0;
	for(qsizetype i = 0; i < m_messages.size(); ++i)
	{
		cursor += m_messages.at(i).rows();
		if(row < cursor)
		{
			return static_cast<int>(i);
		}
	}
	return -1;
}

std::optional<std::pair<quint64, quint64>> ConversationModel::missingRangeAround(quint64 visibleStart,
                                                                                 quint64 visibleEnd,
                                                                                 qsizetype pageSize) const
{
	if(m_messages.isEmpty() || pageSize <= 0)
	{
		return std::nullopt;
	}
	const qsizetype total = m_messages.size();
	const qsizetype vStart = std::clamp<qsizetype>(static_cast<qsizetype>(visibleStart), 0, total - 1);
	const qsizetype vEnd = std::clamp<qsizetype>(static_cast<qsizetype>(visibleEnd), vStart, total - 1);
	constexpr qsizetype Lookahead = 20;
	const qsizetype scanStart = std::max<qsizetype>(0, vStart - Lookahead);
	const qsizetype scanEnd = std::min<qsizetype>(total - 1, vEnd + Lookahead);

	qsizetype firstMissing = -1;
	qsizetype lastMissing = -1;
	for(qsizetype i = scanStart; i <= scanEnd; ++i)
	{
		if(!m_messages.at(i).materialized)
		{
			if(firstMissing < 0)
			{
				firstMissing = i;
			}
			lastMissing = i;
		}
	}
	if(firstMissing < 0)
	{
		return std::nullopt;
	}

	const qsizetype center = (firstMissing + lastMissing) / 2;
	qsizetype fetchStart = std::max<qsizetype>(0, center - pageSize / 2);
	qsizetype fetchEnd = std::min<qsizetype>(total, fetchStart + pageSize);
	if(fetchEnd - fetchStart < pageSize && fetchStart > 0)
	{
		fetchStart = std::max<qsizetype>(0, fetchEnd - pageSize);
	}
	if(firstMissing < fetchStart)
	{
		fetchStart = firstMissing;
		fetchEnd = std::min<qsizetype>(total, fetchStart + pageSize);
	}
	else if(lastMissing >= fetchEnd)
	{
		fetchEnd = std::min<qsizetype>(total, lastMissing + 1);
		fetchStart = std::max<qsizetype>(0, fetchEnd - pageSize);
	}
	while(fetchStart < fetchEnd && m_messages.at(fetchStart).materialized)
	{
		++fetchStart;
	}
	while(fetchEnd > fetchStart && m_messages.at(fetchEnd - 1).materialized)
	{
		--fetchEnd;
	}
	if(fetchStart >= fetchEnd)
	{
		return std::nullopt;
	}
	return std::pair<quint64, quint64>{static_cast<quint64>(fetchStart), static_cast<quint64>(fetchEnd)};
}

void ConversationModel::materializeRange(quint64 startSeq, const QList<proto::Message>& messages)
{
	if(messages.isEmpty())
	{
		return;
	}
	bool anyChanged = false;
	for(qsizetype i = 0; i < messages.size(); ++i)
	{
		const qsizetype slot = static_cast<qsizetype>(startSeq) + i;
		if(slot < 0 || slot >= m_messages.size())
		{
			break;
		}
		const proto::Message& incoming = messages.at(i);
		if(m_messages.at(slot).materialized && m_messages.at(slot).header.id == incoming.id)
		{
			continue;
		}
		std::optional<proto::MessageLayoutMeta> cachedMeta = m_messages.at(slot).layoutMeta;
		MessageNode node = makeNode(incoming);
		node.layoutMeta = cachedMeta ? std::move(cachedMeta) : std::optional(LayoutEstimator::computeMeta(static_cast<quint64>(slot), incoming));
		QList<Block> rows = buildRows(node);
		for(Block& b : rows)
		{
			b.seq = static_cast<quint64>(slot);
		}
		const qsizetype first = firstRowOf(slot);
		const qsizetype oldRows = m_messages.at(slot).rows();
		m_messages[slot] = std::move(node);
		replaceRows(first, oldRows, rows);
		if(slot + 1 < m_messages.size())
		{
			notifyOpensTurn(first + rows.size());
		}
		anyChanged = true;
	}
	if(anyChanged)
	{
		refreshDerivedState();
	}
}

QStringList ConversationModel::evictSlots(const QList<qsizetype>& indices)
{
	if(indices.isEmpty())
	{
		return {};
	}
	QList<qsizetype> sorted = indices;
	std::ranges::sort(sorted, std::greater<>{});
	QStringList evictedIds;
	for(const qsizetype slot : sorted)
	{
		if(slot < 0 || slot >= m_messages.size())
		{
			continue;
		}
		MessageNode& node = m_messages[slot];
		if(!node.materialized || !isEnded(node.header))
		{
			continue;
		}
		if(!node.layoutMeta)
		{
			node.layoutMeta = LayoutEstimator::computeMeta(static_cast<quint64>(slot), materializeHeaderAndParts(node));
		}
		evictedIds.append(node.header.id);
		const qsizetype first = firstRowOf(slot);
		const qsizetype oldRows = node.rows();
		const Block skeleton = makeSkeletonBlock(*node.layoutMeta);
		node = makePlaceholderNode(*node.layoutMeta);
		replaceRows(first, oldRows, {skeleton});
		if(slot + 1 < m_messages.size())
		{
			notifyOpensTurn(first + 1);
		}
	}
	if(!evictedIds.isEmpty())
	{
		refreshDerivedState();
		emit messagesEvicted(evictedIds);
	}
	return evictedIds;
}

QStringList ConversationModel::evictOutside(quint64 visibleStart, quint64 visibleEnd)
{
	if(m_messages.isEmpty())
	{
		return {};
	}
	const qsizetype total = m_messages.size();
	const qsizetype vStart = std::clamp<qsizetype>(static_cast<qsizetype>(visibleStart), 0, total - 1);
	const qsizetype vEnd = std::clamp<qsizetype>(static_cast<qsizetype>(visibleEnd), vStart, total - 1);
	const qsizetype keepStart = std::max<qsizetype>(0, vStart - EvictMargin);
	const qsizetype keepEnd = std::min<qsizetype>(total - 1, vEnd + EvictMargin);

	QList<qsizetype> toEvict;
	QList<qsizetype> remaining;
	for(qsizetype i = 0; i < total; ++i)
	{
		const MessageNode& node = m_messages.at(i);
		if(!node.materialized || !isEnded(node.header))
		{
			continue;
		}
		if(i < keepStart || i > keepEnd)
		{
			toEvict.append(i);
		}
		else if(i < vStart || i > vEnd)
		{
			remaining.append(i);
		}
	}

	const qsizetype residentAfterFirstPass = residentCount() - toEvict.size();
	if(residentAfterFirstPass > MaxResidentMessages && !remaining.isEmpty())
	{
		const qsizetype midpoint = (vStart + vEnd) / 2;
		std::ranges::sort(remaining, [midpoint](qsizetype a, qsizetype b) { return std::abs(a - midpoint) > std::abs(b - midpoint); });
		const qsizetype excess = std::min(residentAfterFirstPass - MaxResidentMessages, remaining.size());
		for(qsizetype i = 0; i < excess; ++i)
		{
			toEvict.append(remaining.at(i));
		}
	}
	return evictSlots(toEvict);
}

QStringList ConversationModel::evictOldTail(qsizetype keepCount)
{
	if(m_messages.size() <= keepCount)
	{
		return {};
	}
	const qsizetype cutoff = std::max<qsizetype>(0, m_messages.size() - keepCount);
	QList<qsizetype> toEvict;
	for(qsizetype i = 0; i < cutoff; ++i)
	{
		const MessageNode& node = m_messages.at(i);
		if(node.materialized && isEnded(node.header))
		{
			toEvict.append(i);
		}
	}
	return evictSlots(toEvict);
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
	QList<Block> rows;
	const bool isAssistant = node.header.role == proto::Role::Assistant;
	for(qsizetype i = 0; isAssistant && i < node.parts.size(); ++i)
	{
		PartNode& part = node.parts[i];
		part.streaming = isPartStreaming(node.header, part.part, i == node.parts.size() - 1);
		part.splitter = {};
		const QList<Block> blocks = assistantPartBlocks(node.header, i, part.part, part.streaming, part.splitter);
		part.rows = blocks.size();
		node.partRows += part.rows;
		rows += blocks;
	}
	const QList<Block> own = ownBlocks(node);
	node.ownRows = own.size();
	return rows + own;
}

QList<Block> ConversationModel::ownBlocks(const MessageNode& node)
{
	const QList<Block> content = node.header.role == proto::Role::User ? userBlocks(node.header, partsOf(node.parts)) : errorBlocks(node.header);
	return content + metaBlocks(node.header);
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

	const QList<Block> rows = ownBlocks(node);
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
	const qsizetype oldRows = m_messages.at(messageIndex).rows();
	const bool wasPlaceholder = !m_messages.at(messageIndex).materialized;
	const qsizetype oldPartRows = m_messages.at(messageIndex).partRows;
	const qsizetype oldOwnRows = m_messages.at(messageIndex).ownRows;

	MessageNode node = makeNode(std::move(message));
	QList<Block> rows = buildRows(node);
	for(Block& b : rows)
	{
		b.seq = static_cast<quint64>(messageIndex);
	}
	// Parts and the rows after them are diffed apart, so a new part is inserted between them
	// instead of turning the last row into something else.
	const qsizetype partRows = node.partRows;
	m_messages[messageIndex] = std::move(node);
	if(wasPlaceholder)
	{
		replaceRows(first, oldRows, rows);
	}
	else
	{
		replaceRows(first, oldPartRows, rows.first(partRows));
		replaceRows(first + partRows, oldOwnRows, rows.mid(partRows));
	}
	refreshDerivedState();
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
		// The position in the conversation is not the builders' to say, see refreshContext().
		Block row = fresh.at(i);
		const Block& previous = m_rows.at(first + i);
		row.stale = previous.stale;
		row.contextStart = previous.contextStart;
		if(row.content == previous.content)
		{
			row.markdownCache = previous.markdownCache;
		}
		if(row.tableHeaders == previous.tableHeaders && row.tableRows == previous.tableRows)
		{
			row.tableCache = previous.tableCache;
		}
		const QList<int> roles = changedRoles(previous, row);
		if(roles.isEmpty())
		{
			continue;
		}
		m_rows[first + i] = std::move(row);
		const QModelIndex changed = index(static_cast<int>(first + i));
		emit dataChanged(changed, changed, roles);
	}

	if(fresh.size() > oldCount)
	{
		beginInsertRows({}, static_cast<int>(first + oldCount), static_cast<int>(first + fresh.size()) - 1);
		// A row added inside a message is stale like the rows next to it.
		const bool stale = first + oldCount > 0 && m_rows.at(first + oldCount - 1).stale;
		for(qsizetype i = oldCount; i < fresh.size(); ++i)
		{
			m_rows.insert(first + i, fresh.at(i));
			m_rows[first + i].stale = stale;
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

QVariantList ConversationModel::allImages() const
{
	QVariantList list;
	for(const Block& block : m_rows)
	{
		if(block.kind == Block::Kind::Image || block.kind == Block::Kind::Tool)
		{
			list.append(imagesToVariant(block.images));
		}
	}
	return list;
}

} // namespace refgui::session
