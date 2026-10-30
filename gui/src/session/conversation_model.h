// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include "proto/types.h"
#include "session/blocks.h"
#include "session/fence_splitter.h"

#include <QAbstractListModel>
#include <QSet>

#include <optional>
#include <utility>

namespace refgui::session
{

// The conversation as a flat list of blocks in display order.
//
//   user        "Fix the build"
//   reasoning   "The error says ..."
//   markdown    "The QML module is missing a file:"
//   code        "qt_add_qml_module(...)"        language = "cmake"
//   tool        "bash ctest"                    toolState, output, failed
//   error       "Connection failed: ..."
//   image       "logo.png"                      images
//   meta        the line under a message        time, agent, tokens
//
// A message is split into blocks because each kind has its own delegate (a code
// block needs a dedicated highlighted TextEdit) and animates independently.
//
// The model shows what the session events say: it is fed whole messages, replaced
// parts and text deltas, and keeps the protocol data next to the rows so that a delta
// lands in the right block. Rows are inserted, removed and updated in place; the model
// is only reset when the whole history is replaced, so view transitions stay intact.
class ConversationModel : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		KindRole = Qt::UserRole + 1, // "user", "reasoning", "markdown", "code", "tool", "error" or "image"
		ContentRole,                 // text; for a tool block, its title; for an image, its name
		LanguageRole,                // code blocks: the fence's language, may be empty
		StreamingRole,               // still receiving content; for a tool block, not finished
		OutputRole,                  // tool blocks
		FailedRole,                  // tool blocks
		MessageIdRole,
		PartIndexRole, // index of the part in its message, -1 for a block of the message itself
		SourceRole,    // user blocks, see Block::source
		SenderRole,
		OpensTurnRole, // a user block that starts a new turn, with a gap above it
		TimeRole,      // unix epoch milliseconds
		ToolNameRole,
		PurposeRole,
		ToolStateRole, // the wire name of the tool state
		InputJsonRole, // the arguments, formatted
		RawInputRole,  // the arguments while they are still streaming
		CallIdRole,
		TimeEndRole,   // tool and meta blocks, 0 while unfinished
		RunAtRole,     // delayed tool blocks: when the tool starts
		MetaRole,      // tool blocks: display extras of the tool as a map
		LiveOutputRole,
		ImagesRole,    // list of {id, mime, size, name}
		AgentRole,     // meta blocks from here on
		ModelIdRole,
		VariantRole,
		TokensInRole,
		TokensOutRole,
		InterruptedRole,
		ErroredRole,
		HumanRole,
		StaleRole,        // before the last compaction
		ContextStartRole, // the last compaction, with older messages above it
		ExpandedRole,     // reasoning, tool and compaction blocks: unfolded
		ShowAllRole,      // tool blocks: the whole output instead of a preview
	};

	explicit ConversationModel(QObject* parent = nullptr);

	// What the tool calls ask of the user, for the keys and the status line.
	struct ToolSummary
	{
		qsizetype awaitingPermission = 0;
		QString oldestAwaitingCallId; // the request the keys answer
		bool lastToolDelayed = false; // the newest tool call waits out its start delay

		bool operator==(const ToolSummary&) const = default;
	};
	const ToolSummary& toolSummary() const { return m_toolSummary; }

	// The error of the session that belongs to no message. It is the last row, unless the last
	// message ended with the same error: that one shows it already. An empty text removes it.
	void setTailError(const QString& text);
	const QString& tailError() const { return m_tailError; }

	// Whether reasoning and tool blocks are unfolded unless the user flipped them.
	void setDefaultExpansion(bool reasoning, bool tools);
	bool reasoningExpandedByDefault() const { return m_reasoningExpanded; }
	bool toolsExpandedByDefault() const { return m_toolsExpanded; }
	// Flips the block of the row against the default; the choice stays with the message part.
	Q_INVOKABLE void toggleExpanded(int row);
	Q_INVOKABLE void toggleShowAll(int row);

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

	// Replaces the whole history. The only operation that resets the model.
	void reset(const QList<proto::Message>& messages);
	// Adds older messages in front of the current ones.
	void prependMessages(const QList<proto::Message>& older);

	// Adds the message, or replaces the one with the same id.
	void setMessage(const proto::Message& message);
	// Replaces part `index` of a message, or appends it when `index` is the part count.
	// Returns false when the message is unknown.
	bool setPart(const QString& messageId, qsizetype index, const proto::Part& part);
	// Streams into part `index`. A part not seen yet is created first. Returns false
	// when the message is unknown.
	bool appendDelta(const QString& messageId, qsizetype index, const proto::Delta& delta);
	void removeMessage(const QString& messageId);

	// The message text as the copy button gives it: the text parts, apart by a blank line.
	std::optional<QString> messageText(const QString& messageId) const;
	// The id of the newest assistant message with text, empty when there is none.
	QString lastReplyId() const;
	// The id of the oldest message, empty when there is none.
	QString firstMessageId() const;

	// The text of a text or reasoning part as shown, if there is one.
	std::optional<QString> partText(const QString& messageId, qsizetype index) const;
	// Number of parts of a message, -1 when it is unknown.
	qsizetype partCount(const QString& messageId) const;

signals:
	void toolSummaryChanged();

private:
	struct PartNode
	{
		proto::Part part;
		qsizetype rows = 0;
		bool streaming = false;
		FenceSplitter splitter{}; // follows the text of a streaming text part
	};

	struct MessageNode
	{
		MessageHeader header; // `parts` stays empty, see `parts` below
		QList<PartNode> parts;
		qsizetype partRows = 0;
		// Rows that belong to no part: the error of an assistant message, or all rows
		// of a user message.
		qsizetype ownRows = 0;

		qsizetype rows() const
		{
			return partRows + ownRows;
		}
	};

	static MessageNode makeNode(proto::Message message);

	qsizetype indexOf(const QString& messageId) const;
	qsizetype firstRowOf(qsizetype messageIndex) const;
	qsizetype firstRowOfPart(qsizetype messageIndex, qsizetype partIndex) const;

	// Builds the rows of a message, filling in the row counts.
	static QList<Block> buildRows(MessageNode& node);
	void rebuildPart(qsizetype messageIndex, qsizetype partIndex);
	void rebuildOwnRows(qsizetype messageIndex);
	// Rebuilds what a change elsewhere in the message made stale: parts whose
	// streaming state changed, and the error.
	void refresh(qsizetype messageIndex);
	void replaceMessage(qsizetype messageIndex, proto::Message message);
	void feedPart(qsizetype messageIndex, qsizetype partIndex, const QString& text);
	void updateAfterPartChange(qsizetype messageIndex, qsizetype partIndex);

	// Replaces `oldCount` rows from `first` by `fresh`, touching only what differs.
	void replaceRows(qsizetype first, qsizetype oldCount, const QList<Block>& fresh);
	void notifyOpensTurn(qsizetype row);

	// The rows of a message after its parts: the error of an assistant message, or the
	// text and images of a user message; then the meta line.
	static QList<Block> ownBlocks(const MessageNode& node);
	// Brings what depends on the whole list up to date: which messages are stale, the tail error.
	void refreshDerivedState();
	void refreshToolSummary();
	void refreshContext();
	void refreshTailError();
	const Block& blockAt(qsizetype row) const;
	bool isExpanded(const Block& block) const;
	void emitRoleChanged(qsizetype row, int role);

	using PartKey = std::pair<QString, qsizetype>; // message id, part index

	QList<MessageNode> m_messages;
	QList<Block> m_rows;
	Block m_tailBlock;
	QString m_tailError;
	ToolSummary m_toolSummary;
	bool m_tailShown = false;
	QSet<PartKey> m_toggled;
	QSet<PartKey> m_shownAll;
	bool m_reasoningExpanded = false;
	bool m_toolsExpanded = false;
};

} // namespace refgui::session
